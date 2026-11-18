//! iOS: the app's home screen over the loading window (`horizon_client::ios_home`). The game's
//! resources load behind it; Play goes on to the title in the same window (an iOS app can't
//! start a second event loop).
use super::*;
use horizon_client::ui_theme::Ui;

pub(super) use horizon_client::ios_home::Launcher;

impl Loading {
    /// The home screen over the window while it shows (iOS): drawing, and its events.
    fn launcher_ui<R>(&self, batch: &mut HudBatch, f: impl FnOnce(&mut Ui) -> R) -> R {
        let atlas = self.gui.as_ref().map_or(&self.boot, |gui| &gui.atlas);
        let cursor = self.launcher.as_ref().map_or((0.0, 0.0), |l| {
            (l.pointer.0 as f32 / batch.scale, l.pointer.1 as f32 / batch.scale)
        });
        let mut ui = Ui::new(atlas, batch, cursor);
        let result = f(&mut ui);
        ui.finish();
        result
    }

    /// Draws the home screen; false when it is closed.
    pub(super) fn draw_launcher(&mut self, batch: &mut HudBatch) -> bool {
        let Some(launcher) = self.launcher.as_ref() else {
            return false;
        };
        let ready = self.loaded.is_some() || self.gui.is_some();
        let _ = launcher;
        self.launcher_ui(batch, |ui| {
            if let Some(launcher) = &self.launcher {
                launcher.draw(ui, ready);
            }
        });
        true
    }

    /// A window event for the home screen; false when it is closed (the menu takes it).
    pub(super) fn launcher_event(&mut self, event: &WindowEvent) -> bool {
        let Some(mut launcher) = self.launcher.take() else {
            return false;
        };
        let size = self
            .renderer
            .as_ref()
            .map_or([1280, 720], |r| [r.config.width, r.config.height]);
        let scale_inputs = self.settings.gui_scale_inputs();
        let window = self.window.clone();
        let play = launcher.event(window.as_deref(), event, |pointer, launcher| {
            let mut batch = HudBatch::new(size, scale_inputs);
            let cursor = (pointer.0 as f32 / batch.scale, pointer.1 as f32 / batch.scale);
            let atlas = self.gui.as_ref().map_or(&self.boot, |gui| &gui.atlas);
            let ui = Ui::new(atlas, &mut batch, cursor);
            let hit = launcher.hit(&ui);
            ui.finish();
            hit
        });
        if !play {
            self.launcher = Some(launcher);
        } else {
            horizon_client::ui_theme::reset_atlas();
        }
        if let Some(window) = &self.window {
            window.request_redraw();
        }
        true
    }
}
