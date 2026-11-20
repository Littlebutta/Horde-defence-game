//! The invite dialog: the host's Steam friends, each with an Invite button. Invites go out through
//! Steam's matchmaking and arrive in the friend's Steam chat, so they work even when the Steam
//! overlay can't open over the game (it often can't when the game isn't started from Steam).
//!
//! The same dialog in join mode lists friends to join: those in a co-op lobby right now, and those
//! whose invite reached the game. Joining from here never depends on Steam handing its "Join"
//! button to the right program (with the test App ID, Steam starts Spacewar when the game is shut).

use super::{fit, App};
use crate::net::Friend;
use crate::render::text;
use crate::ui::{self, alpha, mix, pal, rrect, rrect_lines, RectExt};
use super::widgets::R_CTRL;
use macroquad::prelude::*;

const ROW: f32 = 50.0;

impl App {
    pub(super) fn open_invites(&mut self) {
        self.invite_open = true;
        self.join_mode = false;
        self.invite_scroll = 0.0;
        self.friends_at = -10.0;
    }
    pub(super) fn open_joins(&mut self) {
        self.open_invites();
        self.join_mode = true;
    }

    pub(super) fn invites(&mut self) {
        let join = self.join_mode;
        if is_key_pressed(KeyCode::Escape) || (!join && self.sess.is_none()) || (join && self.steam.is_none()) {
            self.invite_open = false;
            return;
        }
        // the friends list changes as people come online; read it again every couple of seconds
        if self.clock - self.friends_at > 2.0 {
            self.friends_at = self.clock;
            self.friends = if join {
                let mut v = self.steam.as_ref().map(|s| s.friends()).unwrap_or_default();
                // friends who invited us first, then the ones in a lobby
                let inv = &self.got_invites;
                v.sort_by_key(|f| !inv.iter().any(|(w, _)| *w == f.id));
                v
            } else {
                self.sess.as_ref().map(|s| s.friends()).unwrap_or_default()
            };
        }
        let overlay = self.sess.as_ref().is_some_and(|s| s.overlay());
        let p = pal(self.dark);
        self.backdrop();
        let r = ui::screen().centered(600.0, 640.0);
        self.panel(r);
        let mut a = r.pad(30.0, 26.0);
        let mut t = a.cut_top(36.0, 6.0);
        let close = t.cut_right(36.0, 0.0);
        text(&self.fonts.head, if join { "Join a friend" } else { "Invite friends" }, t.x, t.y + 30.0, 30.0, p.text);
        if self.button(close, "×", true, false) {
            self.invite_open = false;
            return;
        }
        let sub = a.cut_top(18.0, 16.0);
        let sub_line = if join {
            "Friends hosting a co-op lobby, or who invited you. Press Join next to their name."
        } else {
            "Invites reach their game and their Steam chat. They can also press Join a friend."
        };
        let line = fit(&self.fonts.mono, sub_line, sub.w, 12.5);
        text(&self.fonts.mono, &line, sub.x, sub.y + 13.0, 12.5, p.dim);

        let mut foot = a.cut_bottom(44.0, 0.0);
        let note = a.cut_bottom(18.0, 12.0);
        let hint: &str = if join {
            "Start Pixel Horde before clicking a Steam chat invite, or Steam opens Spacewar."
        } else if overlay {
            "Or use the Steam overlay's own invite window."
        } else {
            "The Steam overlay isn't running over the game, so invite from here."
        };
        let hint = fit(&self.fonts.mono, hint, note.w, 12.0);
        text(&self.fonts.mono, &hint, note.x, note.y + 13.0, 12.0, p.dim);

        // the list, scrolled with the mouse wheel
        let list = a;
        rrect(list, R_CTRL, alpha(p.panel2, 0.5));
        rrect_lines(list, R_CTRL, 1.0, p.line);
        let inner = list.inset(6.0);
        let friends = std::mem::take(&mut self.friends);
        let rows = friends.len() as f32 * ROW;
        let max = (rows - inner.h).max(0.0);
        if self.hot(list) {
            let (_, wy) = mouse_wheel();
            if wy != 0.0 {
                self.invite_scroll -= wy.signum() * ROW;
            }
        }
        self.invite_scroll = self.invite_scroll.clamp(0.0, max);
        if friends.is_empty() {
            text(&self.fonts.mono, "No Steam friends to show. Is Steam's friends list signed in?", inner.x + 12.0, inner.y + 26.0, 13.0, p.dim);
        }
        let mut pick = None;
        for (i, f) in friends.iter().enumerate() {
            let y = inner.y + i as f32 * ROW - self.invite_scroll;
            if y < inner.y - 1.0 || y + ROW - 6.0 > inner.y + inner.h + 1.0 {
                continue;
            }
            let mut row = Rect::new(inner.x, y, inner.w, ROW - 6.0);
            if self.hot(row) {
                rrect(row, R_CTRL, mix(p.panel2, p.text, 0.05));
            }
            let invite = self.got_invites.iter().rev().find(|(w, _)| *w == f.id).map(|(_, l)| *l);
            let (dot, state) = if join && invite.is_some() {
                (p.good, "INVITED YOU")
            } else if join && f.lobby != 0 {
                (p.good, "IN A CO-OP LOBBY")
            } else {
                status(f, &p)
            };
            draw_circle(row.x + 16.0, row.y + row.h / 2.0, 5.0, dot);
            let btn = row.cut_right(110.0, 0.0).inset(6.0);
            let name = fit(&self.fonts.head, &f.name, row.w - 50.0, 15.0);
            text(&self.fonts.head, &name, row.x + 32.0, row.y + 19.0, 15.0, if f.online { p.text } else { p.dim });
            text(&self.fonts.mono, state, row.x + 32.0, row.y + 36.0, 11.0, p.dim);
            if join {
                let lobby = invite.unwrap_or(f.lobby);
                if self.button(btn, "Join", lobby != 0, lobby != 0) {
                    pick = Some(lobby);
                }
            } else {
                let sent = self.invited.contains(&f.id);
                if self.button(btn, if sent { "Invited ✓" } else { "Invite" }, !sent, f.here && !sent) {
                    pick = Some(f.id);
                }
            }
        }
        self.friends = friends;
        if let (Some(lobby), true) = (pick, join) {
            self.join_lobby(lobby);
            return;
        }
        if let Some(id) = pick {
            let ok = self.sess.as_ref().is_some_and(|s| s.invite_friend(id));
            if ok {
                self.invited.push(id);
            } else {
                self.status = "Steam didn't take that invite. Try again in a moment.".into();
            }
        }

        if overlay && !join && self.button(foot.cut_left(200.0, 0.0), "Steam overlay", true, false) {
            if let Some(s) = &self.sess {
                s.invite();
            }
        }
        if self.button_primary(foot.cut_right(140.0, 0.0), "Done", true) {
            self.invite_open = false;
        }
    }
}

fn status(f: &Friend, p: &ui::Pal) -> (Color, &'static str) {
    if f.here {
        (p.good, "PLAYING PIXEL HORDE")
    } else if f.online {
        (p.accent, "ONLINE")
    } else {
        (alpha(p.dim, 0.6), "OFFLINE")
    }
}
