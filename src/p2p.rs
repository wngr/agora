use std::collections::{BTreeMap, VecDeque};
use std::task::Poll;

use libp2p::{
    core::{
        either::EitherError,
        muxing::StreamMuxerBox,
        transport::{upgrade, Boxed},
    },
    gossipsub::{self, error::GossipsubHandlerError, Gossipsub, GossipsubEvent},
    identity::{self, Keypair},
    mdns::{self, Mdns, MdnsEvent},
    mplex, noise, ping,
    swarm::{
        dial_opts::{DialOpts, PeerCondition},
        NetworkBehaviour, NetworkBehaviourEventProcess, Swarm, SwarmBuilder,
    },
    tcp::TokioTcpConfig,
    NetworkBehaviour, PeerId, Transport,
};
use tracing::debug;

use crate::api::ChatApi;

fn mk_transport() -> (Keypair, Boxed<(PeerId, StreamMuxerBox)>) {
    let keypair = identity::Keypair::generate_ed25519();

    let transport = TokioTcpConfig::new()
        .nodelay(true)
        .upgrade(upgrade::Version::V1)
        .authenticate(
            noise::NoiseConfig::xx(
                noise::Keypair::<noise::X25519Spec>::new()
                    .into_authentic(&keypair)
                    .unwrap(),
            )
            .into_authenticated(),
        )
        .multiplex(mplex::MplexConfig::new())
        .boxed();

    (keypair, transport)
}

pub(crate) type SwarmError =
    EitherError<EitherError<GossipsubHandlerError, void::Void>, ping::Failure>;
#[derive(NetworkBehaviour)]
#[behaviour(
    event_process = true,
    poll_method = "my_poll",
    out_event = "BehaviourEvent"
)]
pub(crate) struct Behaviour {
    pub(crate) gossipsub: Gossipsub,
    mdns: Mdns,
    ping: ping::Ping,

    #[behaviour(ignore)]
    events: VecDeque<NetworkBehaviourAction>,
}

#[derive(Debug)]
pub(crate) enum BehaviourEvent {
    Chat { peer: PeerId, message: ChatApi },
}

impl NetworkBehaviourEventProcess<GossipsubEvent> for Behaviour {
    fn inject_event(&mut self, event: GossipsubEvent) {
        debug!(?event, "GossipSubEvent");
        match event {
            GossipsubEvent::Message {
                propagation_source,
                message,
                ..
            } => {
                let peer = message.source.unwrap_or(propagation_source);
                if let Ok(message) = serde_cbor::from_slice(&message.data) {
                    let ev = BehaviourEvent::Chat { peer, message };
                    self.events
                        .push_back(libp2p::swarm::NetworkBehaviourAction::GenerateEvent(ev));
                }
            }
            GossipsubEvent::Subscribed { .. } => {}
            GossipsubEvent::Unsubscribed { .. } => {}
            GossipsubEvent::GossipsubNotSupported { .. } => {}
        }
    }
}

impl NetworkBehaviourEventProcess<ping::PingEvent> for Behaviour {
    fn inject_event(&mut self, event: ping::PingEvent) {
        debug!(?event, "PingEvent");
    }
}

impl NetworkBehaviourEventProcess<MdnsEvent> for Behaviour {
    fn inject_event(&mut self, event: MdnsEvent) {
        debug!(?event, "MdnsEvent");
        match event {
            MdnsEvent::Discovered(addrs) => {
                let mut addrs_per_peer = BTreeMap::<_, _>::default();
                for (p, a) in addrs {
                    addrs_per_peer.entry(p).or_insert_with(Vec::new).push(a);
                }
                for (p, addrs) in addrs_per_peer {
                    let opts = DialOpts::peer_id(p)
                        .condition(PeerCondition::Disconnected)
                        .addresses(addrs)
                        .build();
                    let ev = libp2p::swarm::NetworkBehaviourAction::Dial {
                        opts,
                        handler: self.new_handler(),
                    };
                    self.events.push_back(ev);
                }
            }
            MdnsEvent::Expired(_) => {}
        }
    }
}
type NetworkBehaviourAction = libp2p::swarm::NetworkBehaviourAction<
    <Behaviour as NetworkBehaviour>::OutEvent,
    <Behaviour as NetworkBehaviour>::ConnectionHandler,
>;

impl Behaviour {
    pub async fn bootstrap() -> anyhow::Result<Swarm<Self>> {
        let (keypair, transport) = mk_transport();
        let peer_id = PeerId::from(keypair.public());
        let mut gossipsub_config = gossipsub::GossipsubConfigBuilder::default();
        gossipsub_config.validation_mode(gossipsub::ValidationMode::Permissive);

        let slf = Self {
            gossipsub: Gossipsub::new(
                gossipsub::MessageAuthenticity::Signed(keypair),
                gossipsub_config.build().unwrap(),
            )
            .unwrap(),
            mdns: Mdns::new(mdns::MdnsConfig::default()).await?,
            ping: ping::Ping::new(ping::Config::new().with_keep_alive(true)),
            events: Default::default(),
        };
        let swarm = SwarmBuilder::new(transport, slf, peer_id)
            .executor(Box::new(|fut| {
                tokio::spawn(fut);
            }))
            .build();
        Ok(swarm)
    }

    fn my_poll(
        &mut self,
        _cx: &mut std::task::Context<'_>,
        _params: &mut impl libp2p::swarm::PollParameters,
    ) -> Poll<NetworkBehaviourAction> {
        if let Some(event) = self.events.pop_front() {
            return Poll::Ready(event);
        }

        Poll::Pending
    }
}
