use super::*;
use crate::macros::wire::wire_str;

const SURFACE: u32 = 4;
const XDG_SURFACE: u32 = 5;
const SYNCOBJ: u32 = 9;
const NOW: u32 = 77;

fn bind(name: u32, iface: &str, id: u32) -> Vec<u8> {
    message(2, 0, &[words(&[name]), wire_str(iface), words(&[1, id])].concat())
}

/// Cage as it starts: a registry (2), xdg_wm_base (3), a surface (4) made a
/// toplevel (6) through its xdg_surface (5), with a syncobj surface (9).
fn cage() -> Shade {
    let mut s = Shade::default();
    for msg in [
        message(DISPLAY, 1, &words(&[2])),
        bind(1, "xdg_wm_base", 3),
        bind(2, "wp_linux_drm_syncobj_manager_v1", 8),
        message(3, GET_XDG_SURFACE, &words(&[XDG_SURFACE, SURFACE])),
        message(XDG_SURFACE, GET_TOPLEVEL, &words(&[6])),
        message(8, GET_SYNCOBJ_SURFACE, &words(&[SYNCOBJ, SURFACE])),
        title("old"),
        title("wlroots"),
    ] {
        assert_eq!(s.request(msg.clone()), [Out::Desktop(msg)]);
    }
    s
}

fn title(name: &str) -> Vec<u8> {
    message(6, SET_TITLE, &wire_str(name))
}

fn attach(buffer: u32) -> Vec<u8> {
    message(SURFACE, ATTACH, &words(&[buffer, 0, 0]))
}

fn frame(id: u32) -> Vec<u8> {
    message(SURFACE, FRAME, &words(&[id]))
}

fn commit_msg() -> Vec<u8> {
    message(SURFACE, COMMIT, &[])
}

fn done(id: u32) -> Vec<u8> {
    message(id, DONE, &words(&[NOW]))
}

fn release(id: u32) -> Vec<u8> {
    message(id, RELEASE, &[])
}

fn configure(serial: u32) -> Vec<u8> {
    message(XDG_SURFACE, CONFIGURE, &words(&[serial]))
}

fn ack(serial: u32) -> Vec<u8> {
    message(XDG_SURFACE, ACK_CONFIGURE, &words(&[serial]))
}

fn desktop(msgs: &[Vec<u8>]) -> Vec<Out> {
    msgs.iter().cloned().map(Out::Desktop).collect()
}

/// Cage drawing a frame: a buffer, the next frame callback, the commit.
fn draw(s: &mut Shade, buffer: u32, callback: u32) -> Vec<Out> {
    [attach(buffer), frame(callback), commit_msg()].into_iter().flat_map(|m| s.request(m)).collect()
}

#[test]
fn a_shown_window_passes_through_untouched() {
    let mut s = cage();
    assert_eq!(draw(&mut s, 20, 30), desktop(&[attach(20), frame(30), commit_msg()]));
    assert_eq!(s.event(done(30), NOW), [Out::Cage(done(30))]);
    assert_eq!(s.event(release(20), NOW), [Out::Cage(release(20))]);
}

#[test]
fn hiding_commits_a_null_buffer_on_cage_s_behalf() {
    let mut s = cage();
    draw(&mut s, 20, 30);
    assert_eq!(s.set_hidden(true, NOW), desktop(&[attach(0), commit_msg()]));
    // The buffer that was up stays the relay's, to show again.
    assert_eq!(s.event(release(20), NOW), []);
}

#[test]
fn a_buffer_cage_has_attached_goes_up_before_the_window_comes_down() {
    let mut s = cage();
    assert_eq!(s.request(attach(20)), desktop(&[attach(20)]));
    assert_eq!(s.set_hidden(true, NOW), [], "never mid-frame");
    assert_eq!(s.request(commit_msg()), desktop(&[commit_msg(), attach(0), commit_msg()]));
}

#[test]
fn while_hidden_cage_s_frames_are_never_answered_and_its_buffers_are_held() {
    let mut s = cage();
    draw(&mut s, 20, 30);
    s.set_hidden(true, NOW);
    assert_eq!(s.event(done(30), NOW), [], "it would only draw for nobody");
    // A buffer attached now replaces the one kept, which is cage's again;
    // a newer one replaces that.
    assert_eq!(
        draw(&mut s, 21, 31),
        [Out::Cage(release(20)), Out::Desktop(frame(31)), Out::Desktop(commit_msg())]
    );
    assert_eq!(s.request(attach(22)), [Out::Cage(release(21))]);
}

#[test]
fn showing_starts_the_window_over_and_puts_the_kept_buffer_back_up() {
    let mut s = cage();
    draw(&mut s, 20, 30);
    s.set_hidden(true, NOW);
    s.event(done(30), NOW);
    assert_eq!(
        s.set_hidden(false, NOW),
        desktop(&[title("wlroots"), commit_msg()]),
        "its name again, and a commit with no buffer"
    );
    // A still client: cage draws nothing, so the relay acks and commits.
    assert_eq!(
        s.event(configure(7), NOW),
        [
            Out::Cage(configure(7)),
            Out::Desktop(ack(7)),
            Out::Desktop(attach(20)),
            Out::Desktop(commit_msg()),
            Out::Cage(done(30)),
        ]
    );
    assert_eq!(s.request(ack(7)), [], "cage's own ack, once is enough");
    assert_eq!(s.request(commit_msg()), desktop(&[commit_msg()]));
    // Shown: frames and buffers are the desktop's to answer again.
    assert_eq!(draw(&mut s, 21, 31), desktop(&[attach(21), frame(31), commit_msg()]));
    assert_eq!(s.event(done(31), NOW), [Out::Cage(done(31))]);
    assert_eq!(s.event(release(20), NOW), [Out::Cage(release(20))]);
}

#[test]
fn showing_puts_up_the_newest_buffer_with_its_sync_points() {
    let mut s = cage();
    draw(&mut s, 20, 30);
    s.set_hidden(true, NOW);
    let acquire = message(SYNCOBJ, SET_ACQUIRE_POINT, &words(&[40, 0, 1]));
    s.request(attach(21));
    assert_eq!(s.request(acquire.clone()), []);
    s.set_hidden(false, NOW);
    let shown = s.event(configure(7), NOW);
    assert_eq!(shown[1..], desktop(&[ack(7), attach(21), acquire, commit_msg()]));
}

#[test]
fn a_frame_the_desktop_has_not_answered_is_left_to_it() {
    let mut s = cage();
    draw(&mut s, 20, 30);
    s.set_hidden(true, NOW);
    s.set_hidden(false, NOW);
    let shown = s.event(configure(7), NOW);
    assert!(!shown.contains(&Out::Cage(done(30))), "{shown:?}");
    assert_eq!(s.event(done(30), NOW), [Out::Cage(done(30))]);
}

#[test]
fn a_window_cage_started_over_while_hidden_goes_straight_back_up() {
    let mut s = cage();
    draw(&mut s, 20, 30);
    s.set_hidden(true, NOW);
    // Cage committed while it was down, and acked the configure it got.
    s.request(commit_msg());
    s.event(configure(7), NOW);
    s.request(ack(7));
    assert_eq!(
        s.set_hidden(false, NOW),
        desktop(&[title("wlroots"), attach(20), commit_msg()]),
        "no second start"
    );
}

#[test]
fn a_configure_cage_has_not_acked_is_acked_for_it() {
    let mut s = cage();
    draw(&mut s, 20, 30);
    s.set_hidden(true, NOW);
    s.request(commit_msg());
    s.event(configure(7), NOW);
    assert_eq!(
        s.set_hidden(false, NOW),
        desktop(&[title("wlroots"), ack(7), attach(20), commit_msg()])
    );
    assert_eq!(s.request(ack(7)), []);
}

#[test]
fn showing_before_the_window_came_down_leaves_it_as_it_was() {
    let mut s = cage();
    s.request(attach(20));
    s.set_hidden(true, NOW);
    assert_eq!(s.set_hidden(false, NOW), []);
    assert_eq!(s.request(commit_msg()), desktop(&[commit_msg()]));
}

#[test]
fn hiding_again_before_the_window_is_back_waits_for_its_configure() {
    let mut s = cage();
    draw(&mut s, 20, 30);
    s.set_hidden(true, NOW);
    s.set_hidden(false, NOW);
    assert_eq!(s.set_hidden(true, NOW), []);
    assert_eq!(s.event(configure(7), NOW), [Out::Cage(configure(7))], "noted, not acted on");
    assert_eq!(
        s.set_hidden(false, NOW),
        desktop(&[title("wlroots"), ack(7), attach(20), commit_msg()])
    );
}

#[test]
fn a_surface_the_desktop_lets_go_of_is_forgotten() {
    let mut s = cage();
    s.event(message(DISPLAY, DELETE_ID, &words(&[SURFACE])), NOW);
    assert_eq!(s.set_hidden(true, NOW), [], "no window left to hide");
}

#[test]
fn a_frame_asked_for_is_answered_once_hidden_or_not() {
    let mut s = cage();
    draw(&mut s, 20, 30);
    s.set_hidden(true, NOW);
    // Hidden: cage waits on 30, which only a copy of the frame answers.
    assert_eq!(s.draw(NOW), [Out::Cage(done(30))]);
    assert_eq!(s.draw(NOW), [], "it has had that one");
    assert_eq!(s.event(done(30), NOW), [], "nor does the desktop's own reach it");
    draw(&mut s, 21, 31);
    s.set_hidden(false, NOW);
    let shown = s.event(configure(7), NOW);
    assert!(!shown.contains(&Out::Cage(done(31))), "the desktop's to answer now");
    // Shown but out of the desktop's sight: answered here, and once.
    assert_eq!(s.draw(NOW), [Out::Cage(done(31))]);
    assert_eq!(s.event(done(31), NOW), []);
}
