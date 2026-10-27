//! Copies one complete measurement into belief, with no model of its own.
//!
//! A mission has two estimator slots, and both default to the name
//! `PassThrough`. In the `localization` slot that name is `PassThroughState`,
//! which copies a state reading (input `state`) into the own-state belief. In
//! the `tracking` slot it is `PassThroughContacts`, which copies a contacts
//! reading (input `contacts`) into the known contacts. They are two structs
//! rather than one because a struct names the readings it needs in its one
//! `Plugin::inputs`, and these two jobs need different readings. With the
//! default perfect measurement each makes belief equal truth at the next
//! decision. Between a slow sensor's readings it keeps the last one.
//!
//! These are the defaults so that a mission can study sensor noise on its own:
//! belief is whatever the last reading said, with no filter in the way. When
//! the estimator is the thing under study, swap in one that predicts or
//! filters, such as the starter crate's DeadReckoning, in the mission's
//! `localization` or `tracking` slot.

use anyhow::{Context, Result};

use crate::KinematicState;
use crate::plugin::{
    ContactList, Input, Localization, LocalizationContext, Plugin, PluginParams, ReadingKind,
    Tracking, TrackingContext, Update,
};

pub struct PassThroughState;

impl Plugin for PassThroughState {
    type Config = ();

    fn configure(params: &PluginParams<'_>) -> Result<()> {
        params.parse()
    }

    fn new(_config: &()) -> Self {
        Self
    }

    fn inputs(_config: &()) -> Vec<Input> {
        vec![Input::new("state", ReadingKind::State)]
    }
}

impl Localization for PassThroughState {
    fn initialize(&mut self, context: &mut LocalizationContext<'_>) -> Result<()> {
        // The initial belief must come from a reading; there is no fallback to truth.
        let reading = context
            .inputs
            .latest::<KinematicState>("state")?
            .context("the state sensor gave no reading at startup")?;
        *context.state = reading.value.clone();
        Ok(())
    }

    fn step(&mut self, context: &mut LocalizationContext<'_>) -> Result<Update> {
        if let Some(reading) = context.inputs.fresh::<KinematicState>("state")? {
            *context.state = reading.value.clone();
        }
        Ok(Update::Applied)
    }
}

pub struct PassThroughContacts;

impl Plugin for PassThroughContacts {
    type Config = ();

    fn configure(params: &PluginParams<'_>) -> Result<()> {
        params.parse()
    }

    fn new(_config: &()) -> Self {
        Self
    }

    fn inputs(_config: &()) -> Vec<Input> {
        vec![Input::new("contacts", ReadingKind::Contacts)]
    }
}

impl Tracking for PassThroughContacts {
    fn initialize(&mut self, context: &mut TrackingContext<'_>) -> Result<()> {
        // No reading yet means no known contacts.
        self.step(context).map(|_| ())
    }

    fn step(&mut self, context: &mut TrackingContext<'_>) -> Result<Update> {
        if let Some(reading) = context.inputs.fresh::<ContactList>("contacts")? {
            *context.contacts = reading.value.clone();
        }
        Ok(Update::Applied)
    }
}
