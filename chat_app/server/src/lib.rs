#![feature(fn_traits)]
#![feature(try_find)]
#![feature(never_type)]

pub mod alias_gen;
pub mod chat_server;
pub mod http_server;
pub mod hub;

use clap::Parser;
use ractor::ActorRef;
use shared::HubMessage;
use std::net::SocketAddr;

use anyhow::anyhow;
use ractor_wormhole::{
    nexus::start_nexus,
    portal::{NexusResult, Portal, PortalActorMessage},
    util::{ActorRef_Ask, ActorRef_Map, FnActor},
};
