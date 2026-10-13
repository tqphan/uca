// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::sync::Arc;

use uca_config::action::{
    Action, AppLaunch, MonitorRef, MouseButton as CfgButton, MouseClick, MouseMove, MouseScroll,
};
use uca_config::values::Length;
use uca_engine::{ActivationId, Intent};
use uca_platform::fake::{FakeCall, FakePlatform};
use uca_types::{Key, KeyDir, MonitorId, MonitorInfo, MouseButton, PhysPoint, RectPhys};

use crate::{Executor, FakeOutput, OutEvent, Pressable};

fn monitors() -> Vec<MonitorInfo> {
    vec![
        MonitorInfo {
            id: MonitorId(0),
            name: "main".into(),
            primary: true,
            bounds_phys: RectPhys::new(0.0, 0.0, 1920.0, 1080.0),
            work_area_phys: RectPhys::new(0.0, 0.0, 1920.0, 1040.0),
            scale_factor: 1.0,
            refresh_hz: 60.0,
        },
        MonitorInfo {
            id: MonitorId(1),
            name: "hidpi".into(),
            primary: false,
            bounds_phys: RectPhys::new(1920.0, 0.0, 3840.0, 2160.0),
            work_area_phys: RectPhys::new(1920.0, 0.0, 3840.0, 2160.0),
            scale_factor: 2.0,
            refresh_hz: 60.0,
        },
    ]
}

fn rig() -> (Executor, FakeOutput, FakePlatform) {
    let out = FakeOutput::default();
    let platform = FakePlatform::new(monitors());
    let exec = Executor::new(Box::new(out.clone()), Arc::new(platform.clone()));
    (exec, out, platform)
}

fn exec(id: u64, actions: Vec<Action>) -> Intent {
    Intent::Execute {
        activation: ActivationId(id),
        actions,
    }
}

use KeyDir::{Down, Up};

/// 09: key.down without key.up, then pause (ReleaseAll) → key.up emitted.
#[test]
fn release_all_releases_unreleased_keys() {
    let (mut e, out, _) = rig();
    e.handle(&exec(1, vec![Action::KeyDown(Key::KeyA)]));
    e.handle(&Intent::ReleaseAll);
    assert_eq!(
        out.take(),
        vec![
            OutEvent::Key(Key::KeyA, Down),
            OutEvent::Key(Key::KeyA, Up),
            OutEvent::ReleaseAll
        ]
    );
    assert!(e.pressed().is_empty());
}

/// 04 rule 4: activation-owned presses are released; key.hold is not.
#[test]
fn activation_owned_release() {
    let (mut e, out, _) = rig();
    e.handle(&exec(
        1,
        vec![Action::KeyDown(Key::ShiftLeft), Action::KeyHold(Key::KeyW)],
    ));
    e.handle(&Intent::Release(ActivationId(1)));
    assert_eq!(
        out.take(),
        vec![
            OutEvent::Key(Key::ShiftLeft, Down),
            OutEvent::Key(Key::KeyW, Down),
            OutEvent::Key(Key::ShiftLeft, Up)
        ]
    );
    assert_eq!(e.pressed(), vec![Pressable::Key(Key::KeyW)]);
    // SAFETY-08 releases the held key too.
    e.handle(&Intent::ReleaseAll);
    assert!(out.take().contains(&OutEvent::Key(Key::KeyW, Up)));
}

#[test]
fn shared_press_is_released_by_the_last_owner() {
    let (mut e, out, _) = rig();
    e.handle(&exec(1, vec![Action::KeyDown(Key::KeyA)]));
    e.handle(&exec(2, vec![Action::KeyDown(Key::KeyA)]));
    e.handle(&Intent::Release(ActivationId(1)));
    assert_eq!(
        out.take(),
        vec![OutEvent::Key(Key::KeyA, Down)],
        "still held by 2"
    );
    e.handle(&Intent::Release(ActivationId(2)));
    assert_eq!(out.take(), vec![OutEvent::Key(Key::KeyA, Up)]);
}

/// 12: hold presses (no-op if held), toggle flips, release releases.
#[test]
fn hold_toggle_release() {
    let (mut e, out, _) = rig();
    e.handle(&exec(
        1,
        vec![Action::KeyHold(Key::KeyW), Action::KeyHold(Key::KeyW)],
    ));
    assert_eq!(out.take(), vec![OutEvent::Key(Key::KeyW, Down)]);
    e.handle(&exec(2, vec![Action::KeyRelease(Key::KeyW)]));
    assert_eq!(out.take(), vec![OutEvent::Key(Key::KeyW, Up)]);
    e.handle(&exec(3, vec![Action::KeyToggle(Key::KeyE)]));
    e.handle(&exec(4, vec![Action::KeyToggle(Key::KeyE)]));
    assert_eq!(
        out.take(),
        vec![OutEvent::Key(Key::KeyE, Down), OutEvent::Key(Key::KeyE, Up)]
    );
}

#[test]
fn mouse_buttons_and_clicks() {
    let (mut e, out, _) = rig();
    e.handle(&exec(
        1,
        vec![
            Action::MouseClick(MouseClick {
                button: CfgButton::Left,
                count: 2,
            }),
            Action::MouseDown(CfgButton::Right),
            Action::MouseScroll(MouseScroll { dx: 0.0, dy: -3.0 }),
        ],
    ));
    e.handle(&Intent::Release(ActivationId(1)));
    assert_eq!(
        out.take(),
        vec![
            OutEvent::Button(MouseButton::Left, Down),
            OutEvent::Button(MouseButton::Left, Up),
            OutEvent::Button(MouseButton::Left, Down),
            OutEvent::Button(MouseButton::Left, Up),
            OutEvent::Button(MouseButton::Right, Down),
            OutEvent::Wheel(-3, 0),
            OutEvent::Button(MouseButton::Right, Up),
        ]
    );
}

#[test]
fn text_launch_and_clipboard() {
    let (mut e, out, platform) = rig();
    e.handle(&exec(
        1,
        vec![
            Action::TextType("Hello world.".into()),
            Action::AppLaunch(AppLaunch {
                path: "notepad.exe".into(),
                args: vec![],
            }),
            Action::ClipboardPaste,
        ],
    ));
    assert_eq!(
        platform.take_calls(),
        vec![
            FakeCall::Text("Hello world.".into()),
            FakeCall::Launch("notepad.exe".into(), vec![])
        ]
    );
    assert_eq!(
        out.take(),
        vec![
            OutEvent::Key(Key::ControlLeft, Down),
            OutEvent::Key(Key::KeyV, Down),
            OutEvent::Key(Key::KeyV, Up),
            OutEvent::Key(Key::ControlLeft, Up),
        ]
    );
}

/// 04 `mouse.move`: logical on a monitor (default: the one with the pointer), or physical.
#[test]
fn mouse_move_spaces() {
    let (mut e, out, platform) = rig();
    e.handle(&exec(
        1,
        vec![Action::MouseMove(MouseMove::Logical {
            x: Length::Percent(50.0),
            y: Length::Percent(50.0),
            relative: false,
            monitor: Some(MonitorRef::Name("hidpi".into())),
        })],
    ));
    assert_eq!(
        platform.take_calls(),
        vec![FakeCall::MoveAbs(PhysPoint::new(3840.0, 1080.0))]
    );
    // Pointer now on the 200 % monitor: a logical 10 px step is 20 physical px.
    e.handle(&exec(
        2,
        vec![Action::MouseMove(MouseMove::Logical {
            x: Length::Px(10.0),
            y: Length::Px(-5.0),
            relative: true,
            monitor: None,
        })],
    ));
    assert_eq!(out.take(), vec![OutEvent::MoveRel(20, -10)]);
    e.handle(&exec(
        3,
        vec![Action::MouseMove(MouseMove::Physical {
            x: 5.0,
            y: 6.0,
            relative: false,
        })],
    ));
    assert_eq!(
        platform.take_calls(),
        vec![FakeCall::MoveAbs(PhysPoint::new(5.0, 6.0))]
    );
}

#[test]
fn debug_actions_go_to_the_app() {
    let (mut e, out, _) = rig();
    let app = e.handle(&exec(
        1,
        vec![Action::DebugOverlay, Action::KeyTap(Key::KeyA)],
    ));
    assert_eq!(app, vec![Action::DebugOverlay]);
    assert_eq!(
        out.take(),
        vec![OutEvent::Key(Key::KeyA, Down), OutEvent::Key(Key::KeyA, Up)]
    );
}

/// 14: switching output device releases everything on the old one first.
#[test]
fn switching_output_releases_first() {
    let (mut e, out, _) = rig();
    e.handle(&exec(1, vec![Action::KeyHold(Key::KeyW)]));
    let new = FakeOutput::default();
    e.set_output(Box::new(new.clone()));
    assert_eq!(
        out.take(),
        vec![
            OutEvent::Key(Key::KeyW, Down),
            OutEvent::Key(Key::KeyW, Up),
            OutEvent::ReleaseAll
        ]
    );
    assert!(new.take().is_empty());
}

/// 11: typed text and typing keys never reach the log; special keys do.
#[test]
fn typed_text_and_keys_are_redacted_in_logs() {
    let (mut e, _, _) = rig();
    let lines = uca_log::capture("uca::actions=debug", || {
        e.handle(&exec(
            1,
            vec![
                Action::TextType("secret".into()),
                Action::KeyTap(Key::KeyQ),
                Action::KeyTap(Key::Enter),
            ],
        ));
    });
    let all = lines.join("\n");
    assert!(!all.contains("secret") && !all.contains("KeyQ"), "{all}");
    assert!(all.contains(r#""text":"<redacted:6 chars>""#), "{all}");
    assert!(all.contains(r#""key":"<redacted key>""#), "{all}");
    assert!(all.contains(r#""key":"Enter""#), "{all}");
}
