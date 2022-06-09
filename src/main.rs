use std::{
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};

use ::libp2p::{futures::StreamExt, gossipsub, swarm::SwarmEvent, Multiaddr};
use anyhow::Context;
use clap::Parser;
use libp2p::{
    gossipsub::{Hasher, Topic},
    PeerId,
};
use tokio::io::{self, AsyncBufReadExt};
use tracing::*;

use p2p::{Behaviour, BehaviourEvent, SwarmError};

mod api;
mod p2p;

/// Chat with your peers
#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// Your name
    #[clap(short, long, default_value_t = random_name())]
    name: String,

    /// Channel to join
    #[clap(short, long, default_value = "agora")]
    channel: String,

    /// Channel to join
    #[clap(short, long)]
    bootstrap: Option<Multiaddr>,
}

fn random_name() -> String {
    names::Generator::default().next().unwrap()
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    tracing_subscriber::fmt::init();
    debug!("{:#?}", args);

    let mut swarm = Behaviour::bootstrap().await?;

    swarm.listen_on("/ip4/0.0.0.0/tcp/0".parse()?)?;

    let topic = gossipsub::IdentTopic::new(args.channel);
    swarm.behaviour_mut().gossipsub.subscribe(&topic)?;

    let mut stdin = io::BufReader::new(io::stdin()).lines();
    let mut state = Default::default();
    let mut ticker = tokio::time::interval(Duration::from_secs(10));
    let msg_nickname = serde_cbor::to_vec(&api::ChatApi::ChangeNickname { nick: args.name })
        .expect("Serialization works");

    loop {
        tokio::select! {
            line = stdin.next_line() => {
                let message = line?.context("stdin closed")?;
                if !message.is_empty() {
                    debug!(?message, ?topic, "gossipsub publish");
                    let msg = api::ChatApi::Message { message, origin_timestamp: chrono::Utc::now() };
                    publish(
                        &mut swarm.behaviour_mut().gossipsub, topic.clone(),
                        &serde_cbor::to_vec(&msg).expect("Serialization works")
                    )?;

                }
            }
            event = swarm.select_next_some() => {
                handle_swarm_event(swarm.behaviour_mut(), &mut state, event)?;
            }
            _ = ticker.tick() => {
                publish(&mut swarm.behaviour_mut().gossipsub, topic.clone(), &*msg_nickname)?;

            }
            _ = tokio::signal::ctrl_c() =>  break
        }
    }

    Ok(())
}

fn publish<S: Hasher>(
    gossipsub: &mut gossipsub::Gossipsub,
    topic: Topic<S>,
    message: &[u8],
) -> anyhow::Result<()> {
    match gossipsub.publish(topic, message) {
        Err(gossipsub::error::PublishError::InsufficientPeers) => println!("No peers available"),
        Err(e) => Err(e)?,
        _ => {}
    }
    Ok(())
}

#[derive(Debug, Default)]
struct State {
    connected_peers: BTreeSet<PeerId>,
    known_nicknames: BTreeMap<PeerId, String>,
}
fn handle_swarm_event(
    _swarm: &mut Behaviour,
    state: &mut State,
    event: SwarmEvent<BehaviourEvent, SwarmError>,
) -> anyhow::Result<()> {
    debug!(?event);
    match event {
        SwarmEvent::Behaviour(ev) => match ev {
            BehaviourEvent::Chat { peer, message } => match message {
                api::ChatApi::Message {
                    message,
                    origin_timestamp,
                } => println!(
                    "{} {}: {}",
                    origin_timestamp,
                    state
                        .known_nicknames
                        .get(&peer)
                        .unwrap_or(&peer.to_string()),
                    message
                ),
                api::ChatApi::ChangeNickname { nick } => {
                    let old = state
                        .known_nicknames
                        .insert(peer, nick.clone())
                        .unwrap_or_else(|| peer.to_string());
                    if old != nick {
                        println!(
                            "{} {} changed his name to {}.",
                            chrono::Utc::now(),
                            old,
                            nick
                        );
                    }
                }
            },
        },
        SwarmEvent::NewListenAddr { address, .. } => {
            info!("Listening on {:?}", address);
        }
        SwarmEvent::ConnectionEstablished { peer_id, .. } => {
            if state.connected_peers.insert(peer_id) {
                // TODO: handle channel joins, not only connections.
                println!(
                    "{} {} connected.",
                    chrono::Local::now(),
                    state
                        .known_nicknames
                        .get(&peer_id)
                        .unwrap_or(&peer_id.to_string())
                );
            }
        }
        SwarmEvent::ConnectionClosed {
            peer_id,
            num_established,
            ..
        } if num_established == 0 => {
            println!(
                "{} {} disconnected.",
                chrono::Local::now(),
                state
                    .known_nicknames
                    .get(&peer_id)
                    .unwrap_or(&peer_id.to_string())
            );
            state.connected_peers.remove(&peer_id);
            // TODO: eventually gc `state.known_nicknames`
        }
        _ => {}
    }
    Ok(())
}
