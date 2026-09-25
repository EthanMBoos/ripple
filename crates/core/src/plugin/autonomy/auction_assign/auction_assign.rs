//! A single random-bid auction initiated by entity 1; no task execution or navigation.
//!
//! C++ counterpart: src/plugins/autonomy/AuctionAssign/AuctionAssign.cpp (LGPL-3.0-or-later).
//! Autonomy phase: consume delivered starts/bids, then publish bids or the final winner.
//! Later autonomy plugins may overwrite this demo's zero velocity commands.
//! Bids come from this plugin's mission-seeded stream, not C++'s shared generator.

use anyhow::{Result, ensure};
use serde::Deserialize;

use crate::plugin::{
    AgentContext, Autonomy, Frame, Plugin, PluginIo, PluginParams, Port, Ports, Unit, Update,
};

pub const START_AUCTION_TOPIC: &str = "StartAuction";
pub const BID_AUCTION_TOPIC: &str = "BidAuction";
pub const RESULT_AUCTION_TOPIC: &str = "ResultAuction";

/// Mission parameters; `Default` supplies any key the mission leaves out.
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AuctionAssignConfig {
    auctioneer: bool,
    #[serde(rename = "auction_duration_s")]
    duration_s: f64,
    #[serde(rename = "network_name")]
    network: String,
}

impl Default for AuctionAssignConfig {
    fn default() -> Self {
        Self {
            auctioneer: true,
            duration_s: 5.0,
            network: "SphereNetwork".into(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AuctionStart {
    pub auctioneer_id: i32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AuctionBid {
    pub bidder_id: i32,
    pub bid: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AuctionResult {
    /// None when no bid reached the auctioneer before it closed.
    pub winner: Option<AuctionBid>,
}

pub struct AuctionAssign {
    auctioneer: bool,
    duration_s: f64,
    network: String,
    started_at_s: Option<f64>,
    completed: bool,
    best_bid: Option<AuctionBid>,
}

impl Plugin for AuctionAssign {
    type Config = AuctionAssignConfig;

    fn configure(params: &PluginParams<'_>) -> Result<AuctionAssignConfig> {
        let config: AuctionAssignConfig = params.parse()?;
        ensure!(
            config.duration_s >= 0.0,
            "auction_duration_s must be nonnegative"
        );
        ensure!(
            !config.network.trim().is_empty(),
            "AuctionAssign network_name must not be empty"
        );
        Ok(config)
    }

    fn new(config: &AuctionAssignConfig) -> Self {
        Self {
            auctioneer: config.auctioneer,
            duration_s: config.duration_s,
            network: config.network.clone(),
            started_at_s: None,
            completed: false,
            best_bid: None,
        }
    }

    fn ports(_config: &AuctionAssignConfig) -> Ports {
        let mut ports = Ports::default();
        for name in ["velocity_x", "velocity_y", "velocity_z"] {
            ports = ports.output(Port::new(name, Unit::MetersPerSecond, Frame::World));
        }
        ports
    }
}

impl Autonomy for AuctionAssign {
    fn initialize(&mut self, context: &mut AgentContext<'_>) -> Result<()> {
        context
            .messages
            .subscribe::<AuctionStart>(&self.network, START_AUCTION_TOPIC)?;
        context
            .messages
            .subscribe::<AuctionBid>(&self.network, BID_AUCTION_TOPIC)?;
        Ok(())
    }

    fn step(&mut self, context: &mut AgentContext<'_>, io: &mut PluginIo) -> Result<Update> {
        for _message in context
            .messages
            .receive::<AuctionStart>(&self.network, START_AUCTION_TOPIC)?
        {
            let bid = AuctionBid {
                bidder_id: context.entity.id,
                bid: context.random.uniform() * 10.0,
            };
            context
                .messages
                .publish(&self.network, BID_AUCTION_TOPIC, bid)?;
        }
        for message in context
            .messages
            .receive::<AuctionBid>(&self.network, BID_AUCTION_TOPIC)?
        {
            // Equal bids retain the first delivered bidder. Completed results never change.
            if !self.completed
                && self
                    .best_bid
                    .is_none_or(|best| message.value.bid > best.bid)
            {
                self.best_bid = Some(*message.value);
            }
        }

        // Matches C++ for now, though we don't consider it correct: only entity ID 1
        // can run the auction; `auctioneer=true` on any other entity does nothing.
        if self.auctioneer && context.entity.id == 1 {
            if let Some(started_at_s) = self.started_at_s {
                // Preserve the C++ strict deadline: close on the first tick after five seconds.
                if !self.completed && context.time.time_s > started_at_s + self.duration_s {
                    // C++ also logged the winner with LOG_INFO. Rust has no plugin log
                    // output yet, so only subscribers see the result; stock run outputs
                    // (events, summary, Rerun) do not record it. Worth adding later.
                    context.messages.publish(
                        &self.network,
                        RESULT_AUCTION_TOPIC,
                        AuctionResult {
                            winner: self.best_bid,
                        },
                    )?;
                    self.completed = true;
                }
            } else {
                context.messages.publish(
                    &self.network,
                    START_AUCTION_TOPIC,
                    AuctionStart {
                        auctioneer_id: context.entity.id,
                    },
                )?;
                self.started_at_s = Some(context.time.time_s);
            }
        }
        for name in ["velocity_x", "velocity_y", "velocity_z"] {
            io.write(name, 0.0)?;
        }
        Ok(Update::Applied)
    }
}
