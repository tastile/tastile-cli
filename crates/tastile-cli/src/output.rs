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
    if !r.pending.is_empty() {
        println!("  pending:     {} items", r.pending.len());
    }
}
