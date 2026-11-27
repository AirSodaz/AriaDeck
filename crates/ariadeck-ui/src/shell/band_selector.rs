//! Mouse marquee (rubber-band) selection and edge auto-scroll for `#download-task-list`.

use std::{collections::HashSet, ops::RangeInclusive, time::Duration};

use gpui::{Bounds, Context, MouseButton, Pixels, Point, Task, Window, px};

use super::{AppPage, AppShell, TASK_ROW_HEIGHT};
use crate::TaskIdentity;

pub(crate) const MARQUEE_DRAG_THRESHOLD_PX: f32 = 5.0;
const EDGE_AUTOSCROLL_ZONE_PX: f32 = 36.0;
const EDGE_AUTOSCROLL_MAX_STEP_PX: f32 = 18.0;
const EDGE_AUTOSCROLL_INTERVAL: Duration = Duration::from_millis(16);
/// Matches `gpui_base::Scrollbar::width()` (`4 * 2 + 8 = 16px`).
const SCROLLBAR_TRACK_WIDTH_PX: f32 = 16.0;

#[derive(Debug)]
pub(crate) struct MarqueeSelectionState {
    pub(crate) origin_window: Point<Pixels>,
    pub(crate) origin_content: Point<Pixels>,
    pub(crate) current_window: Point<Pixels>,
    pub(crate) active: bool,
    pub(crate) ctrl_held: bool,
    pub(crate) shift_held: bool,
    pub(crate) base_selection: HashSet<TaskIdentity>,
    pub(crate) auto_scroll_task: Option<Task<()>>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct MarqueeOverlayGeometry {
    pub(crate) left: f32,
    pub(crate) top: f32,
    pub(crate) width: f32,
    pub(crate) height: f32,
}

/// Map a vertical content-space span `[y1, y2]` to intersecting row indices.
pub(crate) fn band_row_range(
    y1_content: f32,
    y2_content: f32,
    row_height: f32,
    task_count: usize,
) -> Option<RangeInclusive<usize>> {
    if task_count == 0 || row_height <= 0.0 {
        return None;
    }
    let top = y1_content.min(y2_content).max(0.0);
    let bottom = y1_content.max(y2_content);
    if bottom <= 0.0 {
        return None;
    }
    let total_height = task_count as f32 * row_height;
    if top >= total_height {
        return None;
    }
    let start = ((top / row_height).floor() as usize).min(task_count - 1);
    let end = (((bottom - 0.001).max(top) / row_height).floor() as usize).min(task_count - 1);
    (start <= end).then_some(start..=end)
}

/// Compute per-tick vertical scroll delta (in pixels; positive = scroll down, negative = scroll up)
/// when the cursor nears or leaves the top/bottom edge of the list viewport.
pub(crate) fn edge_autoscroll_delta(cursor_y: f32, viewport_top: f32, viewport_bottom: f32) -> f32 {
    if viewport_bottom <= viewport_top {
        return 0.0;
    }
    let effective_zone = EDGE_AUTOSCROLL_ZONE_PX.min((viewport_bottom - viewport_top) * 0.35);
    if effective_zone <= 0.0 {
        return 0.0;
    }
    if cursor_y < viewport_top + effective_zone {
        let ratio = ((viewport_top + effective_zone - cursor_y) / effective_zone).clamp(0.0, 2.0);
        -(ratio * EDGE_AUTOSCROLL_MAX_STEP_PX).max(2.0)
    } else if cursor_y > viewport_bottom - effective_zone {
        let ratio =
            ((cursor_y - (viewport_bottom - effective_zone)) / effective_zone).clamp(0.0, 2.0);
        (ratio * EDGE_AUTOSCROLL_MAX_STEP_PX).max(2.0)
    } else {
        0.0
    }
}

/// Compute the visible marquee rectangle in viewport-local coordinates, clamped to the viewport.
pub(crate) fn marquee_overlay_geometry(
    origin_content: Point<Pixels>,
    current_window: Point<Pixels>,
    viewport: Bounds<Pixels>,
    scroll_offset: Point<Pixels>,
) -> Option<MarqueeOverlayGeometry> {
    let vw = f32::from(viewport.size.width);
    let vh = f32::from(viewport.size.height);
    if vw <= 0.0 || vh <= 0.0 {
        return None;
    }
    let ox = f32::from(origin_content.x).clamp(0.0, vw);
    let oy = (f32::from(origin_content.y) + f32::from(scroll_offset.y)).clamp(0.0, vh);
    let cx = (f32::from(current_window.x) - f32::from(viewport.origin.x)).clamp(0.0, vw);
    let cy = (f32::from(current_window.y) - f32::from(viewport.origin.y)).clamp(0.0, vh);

    let left = ox.min(cx);
    let top = oy.min(cy);
    let right = ox.max(cx);
    let bottom = oy.max(cy);
    Some(MarqueeOverlayGeometry {
        left,
        top,
        width: (right - left).max(1.0),
        height: (bottom - top).max(1.0),
    })
}

/// Combine `base_selection` and the swept `band_range` according to held modifiers:
/// - Default: replace with swept rows
/// - Shift: union `base_selection ∪ band`
/// - Ctrl/Cmd: symmetric toggle `base_selection ^ band`
pub(crate) fn compute_band_selection(
    tasks: &[crate::DownloadRowView],
    base_selection: &HashSet<TaskIdentity>,
    band_range: Option<RangeInclusive<usize>>,
    ctrl_held: bool,
    shift_held: bool,
) -> HashSet<TaskIdentity> {
    let mut band = HashSet::new();
    if let Some(range) = band_range {
        for index in range {
            if let Some(task) = tasks.get(index) {
                band.insert(task.identity.clone());
            }
        }
    }
    if ctrl_held {
        let mut next = base_selection.clone();
        for identity in band {
            if !next.remove(&identity) {
                next.insert(identity);
            }
        }
        next
    } else if shift_held {
        let mut next = base_selection.clone();
        next.extend(band);
        next
    } else {
        band
    }
}

fn has_painted_viewport(viewport: Bounds<Pixels>) -> bool {
    viewport.size.width > px(0.0) && viewport.size.height > px(0.0)
}

impl AppShell {
    pub(crate) fn list_viewport_bounds(&self) -> Bounds<Pixels> {
        self.list_scroll.0.borrow().base_handle.bounds()
    }

    pub(crate) fn list_scroll_offset(&self) -> Point<Pixels> {
        self.list_scroll.0.borrow().base_handle.offset()
    }

    fn can_run_marquee(&self) -> bool {
        self.page == AppPage::Downloads
            && self.active_modal.is_none()
            && !self.snapshot.tasks.is_empty()
    }

    pub(crate) fn begin_marquee_selection(
        &mut self,
        position: Point<Pixels>,
        ctrl_held: bool,
        shift_held: bool,
    ) {
        if !self.can_run_marquee() {
            return;
        }
        let base = self.list_scroll.0.borrow().base_handle.clone();
        let viewport = base.bounds();
        if !has_painted_viewport(viewport) {
            return;
        }
        // Do not initiate marquee drag when pressing inside the vertical scrollbar track.
        if base.max_offset().y > px(0.0)
            && viewport.size.width > px(SCROLLBAR_TRACK_WIDTH_PX)
            && position.x > viewport.right() - px(SCROLLBAR_TRACK_WIDTH_PX)
        {
            return;
        }
        let offset = base.offset();
        let origin_content = Point::new(
            position.x - viewport.origin.x,
            position.y - viewport.origin.y - offset.y,
        );
        self.marquee = Some(MarqueeSelectionState {
            origin_window: position,
            origin_content,
            current_window: position,
            active: false,
            ctrl_held,
            shift_held,
            base_selection: self.selected_tasks.clone(),
            auto_scroll_task: None,
        });
    }

    pub(crate) fn update_marquee_selection(
        &mut self,
        position: Point<Pixels>,
        pressed_button: Option<MouseButton>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.can_run_marquee() {
            self.cancel_marquee_selection();
            return;
        }
        let Some(marquee) = self.marquee.as_mut() else {
            return;
        };
        if pressed_button != Some(MouseButton::Left) {
            let last_pressed_pos = marquee.current_window;
            self.finish_marquee_selection(last_pressed_pos, window, cx);
            return;
        }
        marquee.current_window = position;
        if !marquee.active {
            let dx = f32::from(position.x - marquee.origin_window.x);
            let dy = f32::from(position.y - marquee.origin_window.y);
            if dx.hypot(dy) < MARQUEE_DRAG_THRESHOLD_PX {
                return;
            }
            marquee.active = true;
            self.marquee_consumed_click = true;
            window.focus(&self.focus_handle, cx);
        }

        self.apply_marquee_selection(cx);
        self.ensure_marquee_autoscroll(window, cx);
    }

    pub(crate) fn finish_marquee_selection(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(mut marquee) = self.marquee.take() else {
            return;
        };
        marquee.auto_scroll_task.take();

        if !self.can_run_marquee() {
            self.marquee_consumed_click = false;
            return;
        }

        let viewport = self.list_viewport_bounds();
        if !has_painted_viewport(viewport) {
            self.marquee_consumed_click = false;
            return;
        }

        if marquee.active {
            marquee.current_window = position;
            let offset = self.list_scroll_offset();
            let current_content_y = f32::from(position.y - viewport.origin.y - offset.y);
            let origin_content_y = f32::from(marquee.origin_content.y);
            let dragging_up = current_content_y < origin_content_y;
            let range = band_row_range(
                origin_content_y,
                current_content_y,
                TASK_ROW_HEIGHT,
                self.snapshot.tasks.len(),
            );
            self.selected_tasks = compute_band_selection(
                &self.snapshot.tasks,
                &marquee.base_selection,
                range.clone(),
                marquee.ctrl_held,
                marquee.shift_held,
            );
            self.sync_marquee_anchor_and_focus(range, dragging_up);
            // Reset `marquee_consumed_click` after this mouse-up dispatch cycle finishes
            // so dragging onto empty space never swallows the next single click.
            let shell = cx.entity().downgrade();
            window.defer(cx, move |_, cx| {
                let _ = shell.update(cx, |this, _| {
                    this.marquee_consumed_click = false;
                });
            });
            cx.notify();
            return;
        }

        // Short click (<5px) in empty space below all rows clears selection unless modified.
        let total_height = self.snapshot.tasks.len() as f32 * TASK_ROW_HEIGHT;
        if f32::from(marquee.origin_content.y) >= total_height
            && !marquee.ctrl_held
            && !marquee.shift_held
            && (!self.selected_tasks.is_empty() || self.selected.is_some())
        {
            self.clear_task_selection();
            self.selected = None;
            window.focus(&self.focus_handle, cx);
            cx.notify();
        }
    }

    pub(crate) fn cancel_marquee_selection(&mut self) {
        if let Some(mut marquee) = self.marquee.take() {
            marquee.auto_scroll_task.take();
        }
        self.marquee_consumed_click = false;
    }

    pub(crate) fn active_marquee_overlay(&self) -> Option<MarqueeOverlayGeometry> {
        if !self.can_run_marquee() {
            return None;
        }
        let marquee = self.marquee.as_ref().filter(|m| m.active)?;
        marquee_overlay_geometry(
            marquee.origin_content,
            marquee.current_window,
            self.list_viewport_bounds(),
            self.list_scroll_offset(),
        )
    }

    pub(crate) fn apply_marquee_selection(&mut self, cx: &mut Context<Self>) {
        if !self.can_run_marquee() {
            self.cancel_marquee_selection();
            return;
        }
        let Some(marquee) = self.marquee.as_ref() else {
            return;
        };
        let viewport = self.list_viewport_bounds();
        if !has_painted_viewport(viewport) {
            return;
        }
        let offset = self.list_scroll_offset();
        let origin_content_y = f32::from(marquee.origin_content.y);
        let current_content_y = f32::from(marquee.current_window.y - viewport.origin.y - offset.y);
        let dragging_up = current_content_y < origin_content_y;
        let range = band_row_range(
            origin_content_y,
            current_content_y,
            TASK_ROW_HEIGHT,
            self.snapshot.tasks.len(),
        );
        self.selected_tasks = compute_band_selection(
            &self.snapshot.tasks,
            &marquee.base_selection,
            range.clone(),
            marquee.ctrl_held,
            marquee.shift_held,
        );
        self.sync_marquee_anchor_and_focus(range, dragging_up);
        cx.notify();
    }

    fn sync_marquee_anchor_and_focus(
        &mut self,
        range: Option<RangeInclusive<usize>>,
        dragging_up: bool,
    ) {
        if self.selected_tasks.is_empty() {
            self.range_anchor = None;
            self.selected = None;
            return;
        }
        if let Some(range) = range {
            let (anchor_ix, focus_ix) = if dragging_up {
                (*range.end(), *range.start())
            } else {
                (*range.start(), *range.end())
            };
            if let Some(anchor_task) = self.snapshot.tasks.get(anchor_ix) {
                self.range_anchor = Some(anchor_task.identity.clone());
            }
            if let Some(focus_task) = self.snapshot.tasks.get(focus_ix) {
                self.selected = Some(focus_task.identity.clone());
            }
            return;
        }
        if self
            .selected
            .as_ref()
            .is_some_and(|id| !self.selected_tasks.contains(id))
        {
            self.selected = self
                .snapshot
                .tasks
                .iter()
                .find(|task| self.selected_tasks.contains(&task.identity))
                .map(|task| task.identity.clone());
        }
    }

    fn can_autoscroll_step(&self) -> Option<(f32, f32)> {
        if !self.can_run_marquee() {
            return None;
        }
        let marquee = self.marquee.as_ref().filter(|m| m.active)?;
        let base = self.list_scroll.0.borrow().base_handle.clone();
        let max_scroll = f32::from(base.max_offset().y).max(0.0);
        if max_scroll <= 0.0 {
            return None;
        }
        let viewport = base.bounds();
        let delta = edge_autoscroll_delta(
            f32::from(marquee.current_window.y),
            f32::from(viewport.top()),
            f32::from(viewport.bottom()),
        );
        if delta == 0.0 {
            return None;
        }
        let current_y = f32::from(base.offset().y);
        let next_y = (current_y - delta).clamp(-max_scroll, 0.0);
        if (next_y - current_y).abs() <= f32::EPSILON {
            return None;
        }
        Some((current_y, next_y))
    }

    fn ensure_marquee_autoscroll(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.can_autoscroll_step().is_none() {
            if let Some(marquee) = self.marquee.as_mut() {
                marquee.auto_scroll_task.take();
            }
            return;
        }
        let already_running = self
            .marquee
            .as_ref()
            .is_some_and(|m| m.auto_scroll_task.is_some());
        if already_running {
            return;
        }
        let task = cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(EDGE_AUTOSCROLL_INTERVAL)
                    .await;
                let keep_going = this
                    .update_in(cx, |shell, _window, cx| shell.step_marquee_autoscroll(cx))
                    .unwrap_or(false);
                if !keep_going {
                    break;
                }
            }
        });
        if let Some(marquee) = self.marquee.as_mut() {
            marquee.auto_scroll_task = Some(task);
        }
    }

    fn step_marquee_autoscroll(&mut self, cx: &mut Context<Self>) -> bool {
        let Some((_current_y, next_y)) = self.can_autoscroll_step() else {
            return false;
        };
        let base = self.list_scroll.0.borrow().base_handle.clone();
        let current_offset = base.offset();
        base.set_offset(Point::new(current_offset.x, px(next_y)));
        self.apply_marquee_selection(cx);
        self.can_autoscroll_step().is_some()
    }
}
