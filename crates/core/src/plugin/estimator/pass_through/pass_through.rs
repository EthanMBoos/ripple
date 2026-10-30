//! Copies one complete measurement into belief, with no model of its own.
//!
//! A mission has two estimator slots, and both default to the name
//! `PassThrough`. In the `localization` slot that name is `PassThroughState`,
//! which copies a state reading (input `state`) into the own-state belief. In
//! the `tracking` slot it is `PassThroughContacts`, which copies a contacts
//! reading (input `contacts`) into the known contacts. They are two structs
//! rather than one because a struct names the readings it needs in its one
//! `inputs`, and these two jobs need different readings. With the
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
    ContactList, Input, Localization, LocalizationContext, ReadingKind, Tracking, TrackingContext,
};

pub struct PassThroughState;

impl Localization for PassThroughState {
    type Config = ();

    fn new(_config: &()) -> Self {
        Self
    }

    fn inputs(_config: &()) -> Vec<Input> {
        vec![Input::new("state", ReadingKind::State)]
    }

    fn initialize(&mut self, context: &mut LocalizationContext) -> Result<()> {
        // The initial belief must come from a reading; there is no fallback to truth.
        let reading = context
            .inputs
            .latest::<KinematicState>("state")?
            .context("the state sensor gave no reading at startup")?;
        *context.state = reading.value.clone();
        Ok(())
    }

    fn step(&mut self, context: &mut LocalizationContext) -> Result<()> {
        if let Some(reading) = context.inputs.fresh::<KinematicState>("state")? {
            *context.state = reading.value.clone();
        }
        Ok(())
    }
}

pub struct PassThroughContacts;

impl Tracking for PassThroughContacts {
    type Config = ();

    fn new(_config: &()) -> Self {
        Self
    }

    fn inputs(_config: &()) -> Vec<Input> {
        vec![Input::new("contacts", ReadingKind::Contacts)]
    }

    fn initialize(&mut self, context: &mut TrackingContext) -> Result<()> {
        // No reading yet means no known contacts.
        self.step(context).map(|_| ())
    }

    fn step(&mut self, context: &mut TrackingContext) -> Result<()> {
        if let Some(reading) = context.inputs.fresh::<ContactList>("contacts")? {
            *context.contacts = reading.value.clone();
        }
        Ok(())
    }
}
