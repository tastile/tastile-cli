//! Output formatting helpers.

use tastile_api::{PlacementTileRead, PromptView, SourceTileRead, TileListView};

/// Print a row-aligned table. The CLI uses this for the human-readable
/// output of `tiles`, `today`, `prompts list`, and `source-tiles list`.
pub fn print_table(headers: &[&str], rows: Vec<Vec<String>>) {
    if rows.is_empty() {
        println!("(no rows)");
        return;
    }
    let widths = headers
        .iter()
        .enumerate()
        .map(|(i, h)| {
            let col_width = rows
                .iter()
                .map(|r| r.get(i).map(String::len).unwrap_or(0))
                .max()
                .unwrap_or(0);
            h.len().max(col_width)
        })
        .collect::<Vec<_>>();

    let sep = widths
        .iter()
        .map(|w| "-".repeat(*w))
        .collect::<Vec<_>>()
        .join("  ");
    println!("{sep}");
    let header_line = headers
        .iter()
        .zip(&widths)
        .map(|(h, w)| format!("{h:<w$}"))
        .collect::<Vec<_>>()
        .join("  ");
    println!("{header_line}");
    println!("{sep}");
    for row in rows {
        let line = row
            .iter()
            .zip(&widths)
            .map(|(c, w)| format!("{c:<w$}"))
            .collect::<Vec<_>>()
            .join("  ");
        println!("{line}");
    }
    println!("{sep}");
}

fn short(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

fn unplaced_occurrence_row(
    occurrence_id: Option<&serde_json::Value>,
    blocked: &serde_json::Value,
) -> Option<Vec<String>> {
    let reason = blocked.get("reason")?.as_i64()?;
    let reason = match reason {
        0 => "INSUFFICIENT_CAPACITY".to_string(),
        1 => "LIMIT_CONFLICT".to_string(),
        2 => "PRIORITY_TIE_CONFLICT".to_string(),
        other => format!("UNKNOWN({other})"),
    };
    let field = |name: &str| {
        blocked
            .get(name)
            .and_then(|value| match value {
                serde_json::Value::String(text) => Some(text.clone()),
                serde_json::Value::Number(number) => Some(number.to_string()),
                _ => None,
            })
            .unwrap_or_else(|| "-".to_string())
    };
    let occurrence_id = occurrence_id
        .and_then(serde_json::Value::as_str)
        .unwrap_or("-")
        .to_string();

    Some(vec![
        occurrence_id,
        reason,
        field("required_duration_ms"),
        field("available_duration_ms"),
        field("blocked_at"),
        field("conflicting_source_tile_id"),
    ])
}

fn print_unplaced_occurrences(rows: Vec<Vec<String>>) {
    if !rows.is_empty() {
        println!("Unplaced occurrences:");
        print_table(
            &[
                "occurrence_id",
                "reason",
                "required_ms",
                "available_ms",
                "blocked_at",
                "conflicting_source_id",
            ],
            rows,
        );
    }
}

pub fn print_tile(t: &TileListView) {
    println!("{}", t.title);
    println!("  id:          {}", t.id);
    println!("  lifecycle:   {}", t.lifecycle);
    println!(
        "  worked/break: {}m / {}m",
        t.worked_minutes / 60_000,
        t.break_minutes / 60_000
    );
    if let Some(d) = t.projected_next_start_at {
        println!("  next start:  {}", d.format("%Y-%m-%d %H:%M UTC"));
    }
    if let Some(plan) = t.plan_id {
        println!("  plan:        {plan}");
    }
    if !t.labels.is_empty() {
        println!("  labels:      {}", t.labels.join(", "));
    }
    if let Some(n) = &t.next_action {
        println!("  next action: {n}");
    }
    if let Some(n) = &t.resume_note {
        println!("  resume note: {n}");
    }
}

pub fn print_tiles_table(tiles: &[TileListView]) {
    let mut rows: Vec<Vec<String>> = Vec::with_capacity(tiles.len());
    for t in tiles {
        rows.push(vec![
            t.id.to_string(),
            short(&t.title, 40),
            format!("{}", t.lifecycle),
            format!(
                "{}m / {}m",
                t.worked_minutes / 60_000,
                t.break_minutes / 60_000
            ),
            t.projected_next_start_at
                .map(|d| d.format("%Y-%m-%d %H:%M").to_string())
                .unwrap_or_else(|| "-".into()),
            t.labels.join(","),
        ]);
    }
    print_table(
        &[
            "id",
            "title",
            "lifecycle",
            "worked/break",
            "next start",
            "labels",
        ],
        rows,
    );
}

pub fn print_source_tiles_table(items: &[SourceTileRead]) {
    let mut rows: Vec<Vec<String>> = Vec::with_capacity(items.len());
    for s in items {
        rows.push(vec![
            s.source_tile_id.to_string(),
            short(&s.title, 40),
            format!("{}", s.source_state),
            format!("{}", s.revision),
            format!("{}", s.plan_role),
            s.plan_id.to_string(),
        ]);
    }
    print_table(
        &["id", "title", "state", "revision", "plan_role", "plan_id"],
        rows,
    );
}

pub fn print_source_tile_detail(detail: &tastile_api::SourceTileDetailRead) {
    let s = &detail.source;
    println!("SourceTile {}", s.source_tile_id);
    println!("  title:      {}", s.title);
    println!("  state:      {}  (0=ACTIVE..3=CANCELLED)", s.source_state);
    println!("  plan:       {}  (role={})", s.plan_id, s.plan_role);
    println!("  revision:   {}", s.revision);
    println!("  created_at: {}", s.created_at);
    println!("  updated_at: {}", s.updated_at);
    if let Some(c) = &s.color {
        println!("  color:      {c}");
    }
    if let Some(i) = &s.icon {
        println!("  icon:       {i}");
    }
    if let Some(d) = &s.description {
        println!("  description: {d}");
    }
    println!("  relations:  {} entries", detail.relations.len());
    println!("  occurrences: {} entries", detail.occurrences.len());
    println!("  placements:  {} entries", detail.placements.len());
    let unplaced = detail
        .occurrences
        .iter()
        .filter_map(|occurrence| {
            let blocked = occurrence.0.get("blocked")?;
            if blocked.is_null() {
                return None;
            }
            unplaced_occurrence_row(occurrence.0.get("occurrence_id"), blocked)
        })
        .collect();
    print_unplaced_occurrences(unplaced);
    let schedule = &s.schedule;
    println!(
        "  schedule:    kind={} duration={}ms priority={}",
        schedule.generation.kind, schedule.required_duration_ms, schedule.priority
    );
    if let Some(at) = schedule.generation.at {
        println!("  at:          {}", at);
    }
    if let Some(starts_at) = schedule.generation.starts_at {
        println!("  starts_at:   {}", starts_at);
    }
    if let Some(interval_ms) = schedule.generation.interval_ms {
        println!("  interval_ms: {}", interval_ms);
    }
    if let Some(mask) = schedule.generation.weekday_mask {
        println!("  weekday_mask: {mask}");
    }
    if let Some(offset) = schedule.generation.offset_min {
        println!("  offset_min:  {offset}");
    }
    if !schedule.generation.excluded_dates.is_empty() {
        println!(
            "  excluded:    {}",
            schedule.generation.excluded_dates.join(", ")
        );
    }
    println!(
        "  window:      {}..{} ms after the nominal time",
        schedule.window.start_offset_ms, schedule.window.end_offset_ms
    );
    print_placements_table(&detail.placements);
}

pub fn print_placements_table(items: &[PlacementTileRead]) {
    let mut rows: Vec<Vec<String>> = Vec::with_capacity(items.len());
    for p in items {
        rows.push(vec![
            p.placement_id.to_string(),
            p.source_tile_id.to_string(),
            p.start.format("%Y-%m-%d %H:%M").to_string(),
            p.end.format("%Y-%m-%d %H:%M").to_string(),
            format!("{}/{}", p.split_index, p.split_count),
            if p.closed { "closed" } else { "open" }.to_string(),
        ]);
    }
    print_table(
        &[
            "placement_id",
            "source_id",
            "start",
            "end",
            "split",
            "state",
        ],
        rows,
    );
}

pub fn print_prompts_table(items: &[PromptView]) {
    let mut rows: Vec<Vec<String>> = Vec::with_capacity(items.len());
    for p in items {
        rows.push(vec![
            p.id.to_string(),
            format!("{}", p.kind),
            p.created_at.format("%Y-%m-%d %H:%M").to_string(),
            p.expires_at
                .map(|d| d.format("%Y-%m-%d %H:%M").to_string())
                .unwrap_or_else(|| "-".into()),
            p.note.clone().unwrap_or_else(|| "-".into()),
        ]);
    }
    print_table(&["id", "kind", "created_at", "expires_at", "note"], rows);
}

pub fn print_command_response(r: &tastile_api::CommandResponse) {
    println!("  command_id:  {}", r.command_id);
    println!("  accepted_at: {}", r.accepted_at);
    if let Some(rev) = r.revision {
        println!("  revision:    {rev}");
    }
    println!("  result:      {}", r.result);
    if let Some(aggregate) = &r.aggregate {
        if let Some(id) = aggregate.get("id").and_then(|value| value.as_str()) {
            println!("  aggregate:   {id}");
        }
    }
    if !r.pending.is_empty() {
        println!("  pending:     {} items", r.pending.len());
    }
    let unplaced = r
        .aggregate_meta
        .as_ref()
        .and_then(|metadata| metadata.get("unplaced_occurrences"))
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| unplaced_occurrence_row(item.get("occurrence_id"), item))
                .collect()
        })
        .unwrap_or_default();
    print_unplaced_occurrences(unplaced);
}

/// Print the field-level diff between the stored source tile and a
/// proposed draft.
pub fn print_diff(diffs: &[crate::app::FieldDiff]) {
    if diffs.is_empty() {
        println!("No field changes detected by the read model.");
        return;
    }
    let mut rows: Vec<Vec<String>> = Vec::with_capacity(diffs.len());
    for d in diffs {
        rows.push(vec![
            d.field.clone(),
            short(&d.current, 40),
            short(&d.proposed, 40),
        ]);
    }
    print_table(&["field", "current", "proposed"], rows);
}

/// Print the placement scope a cancel / reflow would touch.
pub fn print_impact(operation: &str, impact: &crate::app::PlacementImpact) {
    println!(
        "{operation} impact: {} placement(s) owned, {} still open, {} in scope",
        impact.total, impact.open, impact.in_range
    );
    if !impact.rows.is_empty() {
        print_placements_table(&impact.rows);
    }
}

/// Print the state read back from the server after a write.
pub fn print_readback(state: &crate::app::SourceTileState) {
    println!(
        "  readback:    state={} revision={} placements={} (open {})",
        state.source_state, state.revision, state.placements_total, state.placements_open
    );
}

/// Print timeline items ordered as the server returns them
/// (`span.start ASC, placement_id ASC`). LABEL (`role = 1`) rows are
/// non-blocking label periods and are counted separately — they never
/// count as busy time (v1/14 §4).
pub fn print_timeline(items: &[tastile_api::TimelineItem]) {
    if items.is_empty() {
        println!("(no placements in range)");
        return;
    }
    let mut rows: Vec<Vec<String>> = Vec::with_capacity(items.len());
    let mut label_count = 0usize;
    for item in items {
        if item.role == 1 {
            label_count += 1;
        }
        rows.push(vec![
            item.span.start.format("%Y-%m-%d %H:%M").to_string(),
            item.span.end.format("%H:%M").to_string(),
            timeline_role(item),
            timeline_title(item),
            timeline_state(item),
            timeline_notes(item),
        ]);
    }
    print_table(&["start", "end", "role", "title", "state", "notes"], rows);
    if label_count > 0 {
        println!(
            "{label_count} LABEL item(s) are non-blocking label periods; they never count as busy time."
        );
    }
}

fn timeline_role(item: &tastile_api::TimelineItem) -> String {
    match item.role {
        0 => "EXEC".to_string(),
        1 => "LABEL".to_string(),
        other => format!("role={other}"),
    }
}

fn timeline_title(item: &tastile_api::TimelineItem) -> String {
    if item.inside.is_some() {
        format!("  {}", item.content.title)
    } else {
        item.content.title.clone()
    }
}

fn timeline_state(item: &tastile_api::TimelineItem) -> String {
    match item.resolution.state {
        0 => "open".to_string(),
        1 => "closed".to_string(),
        2 => format!("BLOCKED({})", item.resolution.violations.len()),
        other => format!("state={other}"),
    }
}

fn timeline_notes(item: &tastile_api::TimelineItem) -> String {
    let mut notes = Vec::new();
    if item.inside.is_some() {
        notes.push("nested".to_string());
    }
    if let (Some(split_index), Some(split_count)) = (item.split_index, item.split_count) {
        if split_count > 1 {
            notes.push(format!("split {split_index}/{split_count}"));
        }
    }
    match item.source.kind {
        0 => notes.push("manual".to_string()),
        1 => notes.push("recurring".to_string()),
        2 => notes.push("flow".to_string()),
        3 => notes.push("import".to_string()),
        4 => notes.push("source".to_string()),
        other => notes.push(format!("source={other}")),
    }
    if let Some(owner) = &item.owner {
        notes.push(format!("owner:{}", owner.profile.display_name));
    }
    notes.join(", ")
}
