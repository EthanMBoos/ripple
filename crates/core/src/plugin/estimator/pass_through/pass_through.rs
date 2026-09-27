//! Copies one complete measurement into belief, with no model of its own.
//!
//! As localization it copies a state reading (input `state`); as tracking it
//! copies a contacts reading (input `contacts`). With the default perfect
//! measurement this makes belief equal truth at the next decision. Between a
//! slow sensor's readings it keeps the last one.

use anyhow::{Context, Result};

use crate::KinematicState;
use crate::plugin::{
    ContactList, Input, Localization, LocalizationContext, Plugin, PluginParams, ReadingKind,
    Tracking, TrackingContext, Update,
};

pub struct PassThrough;

impl Plugin for PassThrough {
    type Config = ();

    fn configure(params: &PluginParams<'_>) -> Result<()> {
        params.parse()
    }

    fn new(_config: &()) -> Self {
        Self
    }
}

impl Localization for PassThrough {
    fn inputs(_config: &()) -> Vec<Input> {
        vec![Input::new("state", ReadingKind::State)]
    }

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

impl Tracking for PassThrough {
    fn inputs(_config: &()) -> Vec<Input> {
        vec![Input::new("contacts", ReadingKind::Contacts)]
    }

    fn initialize(&mut self, context: &mut TrackingContext<'_>) -> Result<()> {
        // No reading yet means no known contacts.
        Tracking::step(self, context).map(|_| ())
    }

    fn step(&mut self, context: &mut TrackingContext<'_>) -> Result<Update> {
        if let Some(reading) = context.inputs.fresh::<ContactList>("contacts")? {
            *context.contacts = reading.value.clone();
        }
        Ok(Update::Applied)
    }
}
