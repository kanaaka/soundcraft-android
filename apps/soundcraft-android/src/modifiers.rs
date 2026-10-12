//! Modifier keys, tracked by the shell because the platform never reports them.
//!
//! # The gap
//!
//! winit 0.30.13's Android backend emits exactly six window events — `Focused`, `Resized`,
//! `ScaleFactorChanged`, `RedrawRequested`, `Touch` and `KeyboardInput`
//! (`src/platform_impl/android/mod.rs`) — and **never** `WindowEvent::ModifiersChanged`. egui-winit
//! updates its modifier state from that one event and nowhere else, and egui 0.36 carries
//! modifiers as an *event* (`RawInput` has no `modifiers` field any more), so on Android
//! `ctx.input(|i| i.modifiers)` is the all-false default for the whole run:
//!
//! - Shift+click selects a single cell instead of extending the selection,
//! - Ctrl+C / Ctrl+V / Ctrl+S and every other modifier shortcut does nothing,
//! - Alt keytips never come up.
//!
//! The *key events themselves* do arrive, so the modifiers can be tracked from them — which is what
//! a desktop backend does internally. This module does that and reports the state the way egui
//! expects: `Event::ModifiersChanged` (egui's `InputState` reads modifiers from that event alone)
//! plus the per-event `modifiers` copies that `Event::Key` and `Event::PointerButton` carry.
//!
//! Call [`Tracked::apply`] from the shell's `eframe::App::raw_input_hook`, *before* forwarding to
//! the app's own hook, so the app sees the same frames it would on the desktop.
//!
//! # What it does not do
//!
//! The Super/Windows key is deliberately not tracked: on Windows and Linux egui's `command`
//! modifier is Ctrl and `mac_cmd` is documented as macOS-only, so there is no modifier here for it
//! to become. Alt is tracked and reported — it is what opens keytips — but egui's own note that
//! Alt+Shift is how macOS types special characters does not apply on Android.

/// The three modifiers that mean something on this platform, held between frames.
///
/// Between frames because the key press and the click are usually in *different* frames: the user
/// holds Shift down, and only then taps a cell.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tracked {
    alt: bool,
    ctrl: bool,
    shift: bool,
}

impl Tracked {
    /// Fold this frame's events into the tracked state and report it back into them.
    pub fn apply(&mut self, raw: &mut egui::RawInput) {
        let events = std::mem::take(&mut raw.events);
        let mut out = Vec::with_capacity(events.len() + 1);
        for mut event in events {
            self.observe(&event);
            // Every event that carries a copy gets the state as it stands at that point in the
            // batch, so a Shift press and a click in the same frame are ordered correctly.
            let modifiers = self.reported();
            match &mut event {
                egui::Event::Key { modifiers: m, .. } | egui::Event::PointerButton { modifiers: m, .. } => *m = modifiers,
                _ => {}
            }
            out.push(event);
        }
        // egui's `InputState` takes modifiers from this event and nowhere else, and a frame with no
        // key events in it — the vast majority — still has to report the state it is holding.
        out.push(egui::Event::ModifiersChanged(self.reported()));
        raw.events = out;
    }

    /// The state as egui wants to see it.
    fn reported(&self) -> egui::Modifiers {
        // On Windows and Linux the shortcut modifier is Ctrl, and `mac_cmd` must stay off there.
        egui::Modifiers { alt: self.alt, ctrl: self.ctrl, shift: self.shift, mac_cmd: false, command: self.ctrl }
    }

    /// Update from one event.
    fn observe(&mut self, event: &egui::Event) {
        match event {
            // A modifier key going down or coming up. Physical keys: winit reports the left and
            // right keys separately, and either of them is "the" modifier.
            egui::Event::Key { key, pressed, .. } => match key {
                egui::Key::ShiftLeft | egui::Key::ShiftRight => self.shift = *pressed,
                egui::Key::ControlLeft | egui::Key::ControlRight => self.ctrl = *pressed,
                egui::Key::AltLeft | egui::Key::AltRight => self.alt = *pressed,
                _ => {}
            },
            // The platform reported them itself. It never does on Android, but egui-winit
            // synthesises one when the window loses focus, and that report is authoritative: it is
            // how a modifier that was held while the app went to the background stops being stuck.
            egui::Event::ModifiersChanged(reported) => {
                self.shift = reported.shift;
                self.ctrl = reported.ctrl;
                self.alt = reported.alt;
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Tracked;
    use egui::{Event, Key, Modifiers, PointerButton, Pos2, RawInput};

    fn frame(events: Vec<Event>) -> RawInput {
        RawInput { events, ..Default::default() }
    }

    fn key(key: Key, pressed: bool) -> Event {
        Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE }
    }

    fn click() -> Event {
        Event::PointerButton { pos: Pos2::ZERO, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE }
    }

    /// The modifiers egui will see on the click in this frame.
    fn on_click(raw: &RawInput) -> Modifiers {
        raw.events
            .iter()
            .find_map(|e| match e {
                Event::PointerButton { modifiers, .. } => Some(*modifiers),
                _ => None,
            })
            .expect("this frame has a click")
    }

    /// The modifiers egui will see for this key.
    fn on_key(raw: &RawInput, key: Key) -> Modifiers {
        raw.events
            .iter()
            .find_map(|e| match e {
                Event::Key { key: k, modifiers, .. } if *k == key => Some(*modifiers),
                _ => None,
            })
            .expect("this frame has that key")
    }

    /// The last `ModifiersChanged` — the event egui's `InputState` actually reads.
    fn reported(raw: &RawInput) -> Modifiers {
        raw.events
            .iter()
            .rev()
            .find_map(|e| match e {
                Event::ModifiersChanged(m) => Some(*m),
                _ => None,
            })
            .expect("a ModifiersChanged is always appended")
    }

    #[test]
    fn every_frame_reports_the_held_state() {
        let mut t = Tracked::default();
        let mut f = frame(vec![]);
        t.apply(&mut f);
        assert_eq!(reported(&f), Modifiers::NONE);

        let mut f = frame(vec![key(Key::ShiftLeft, true)]);
        t.apply(&mut f);
        assert!(reported(&f).shift);
    }

    #[test]
    fn a_click_in_a_later_frame_still_carries_the_held_modifier() {
        // The case that matters and the one a single-frame implementation gets wrong: the user
        // holds Shift down, and only then taps.
        let mut t = Tracked::default();
        let mut down = frame(vec![key(Key::ShiftLeft, true)]);
        t.apply(&mut down);

        let mut later = frame(vec![click()]);
        t.apply(&mut later);
        assert!(on_click(&later).shift, "the click must extend, not select");
    }

    #[test]
    fn ctrl_s_is_a_command_shortcut() {
        let mut t = Tracked::default();
        // Ctrl and S in the same frame, in the order the platform sends them.
        let mut f = frame(vec![key(Key::ControlLeft, true), key(Key::S, true)]);
        t.apply(&mut f);
        let m = on_key(&f, Key::S);
        assert!(m.ctrl, "Ctrl is what egui calls `ctrl` on this platform");
        assert!(m.command, "and `command` is what a shortcut matches against");
        assert!(!m.mac_cmd);
    }

    #[test]
    fn a_modifier_going_up_stops_being_held() {
        let mut t = Tracked::default();
        let mut f = frame(vec![key(Key::AltLeft, true)]);
        t.apply(&mut f);
        assert!(reported(&f).alt);
        let mut f = frame(vec![key(Key::AltLeft, false)]);
        t.apply(&mut f);
        assert!(!reported(&f).alt);
    }

    #[test]
    fn a_platform_report_overrides_what_was_tracked() {
        // egui-winit synthesises one of these when the window loses focus, and it is
        // authoritative: otherwise a modifier held while the app went to the background stays
        // stuck down for the rest of the run.
        let mut t = Tracked::default();
        let mut f = frame(vec![key(Key::ControlLeft, true)]);
        t.apply(&mut f);
        assert!(reported(&f).ctrl);

        let mut f = frame(vec![Event::ModifiersChanged(Modifiers::NONE)]);
        t.apply(&mut f);
        assert!(!reported(&f).ctrl, "the platform's own report wins");
    }
}
