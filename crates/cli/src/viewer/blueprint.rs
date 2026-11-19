//! Simulation-first Rerun blueprint: a dominant agent map with compact debug panels.
use anyhow::Result;
use rerun::{
    RecordingStream,
    blueprint::{
        Blueprint, BlueprintActivation, BlueprintPanel, Horizontal, SelectionPanel, Spatial2DView,
        Spatial3DView, Tabs, TextDocumentView, TimePanel, TimeSeriesView, Vertical,
        components::{LoopMode, PanelState, PlayState},
    },
};

pub(super) fn send(recording: &RecordingStream) -> Result<()> {
    let map = Tabs::new([
        Spatial2DView::new("Mission map · all agents")
            .with_origin("map")
            .into(),
        Spatial3DView::new("3D world").with_origin("world").into(),
    ]);
    let debug = Vertical::new([
        TextDocumentView::new("Mission status")
            .with_origin("dashboard")
            .into(),
        TimeSeriesView::new("Agent speeds · m/s")
            .with_origin("debug/speed_mps")
            .into(),
        TimeSeriesView::new("Agent altitudes · m")
            .with_origin("debug/altitude_m")
            .into(),
    ])
    .with_row_shares(vec![1.2, 1.0, 1.0]);
    let layout = Horizontal::new([map.into(), debug.into()]).with_column_shares(vec![4.0, 1.0]);
    Blueprint::new(layout)
        .with_auto_views(false)
        .with_auto_layout(false)
        .with_blueprint_panel(BlueprintPanel::new().with_state(PanelState::Collapsed))
        .with_selection_panel(SelectionPanel::new().with_state(PanelState::Collapsed))
        .with_time_panel(
            TimePanel::new()
                .with_state(PanelState::Collapsed)
                .with_timeline("time")
                .with_play_state(PlayState::Playing)
                .with_loop_mode(LoopMode::All)
                .with_playback_speed(1.0),
        )
        .send(recording, BlueprintActivation::default())?;
    Ok(())
}
