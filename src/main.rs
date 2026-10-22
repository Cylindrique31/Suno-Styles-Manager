#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use arboard::Clipboard;
use eframe::egui;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, path::PathBuf, time::Duration};
use serde_json::Value;

#[derive(Clone, Debug, Serialize, Deserialize)]
struct AppSettings {
    language: String,
    theme: String,
    #[serde(default)]
    add_suno_link: bool,
}
impl Default for AppSettings {
    fn default() -> Self { Self { language: "Français".into(), theme: "Même que l’OS".into(), add_suno_link: false } }
}

#[derive(Clone, Debug, Default)]
struct SunoStyle {
    id: i64,
    name: String,
    tags: String,
    music_style: String,
    style: String,
    exclusion: String,
    author: String,
    suno_url: String,
    favorite: bool,
    created_at: String,
}


#[derive(Clone, Debug, Serialize, Deserialize)]
struct PortableStyle {
    #[serde(default)]
    name: String,
    #[serde(default)]
    tags: String,
    #[serde(default)]
    music_style: String,
    #[serde(default)]
    style: String,
    #[serde(default)]
    exclusion: String,
    #[serde(default)]
    author: String,
    #[serde(default)]
    suno_url: String,
    #[serde(default)]
    favorite: bool,
}

// Les noms servent d'identifiant fonctionnel lors de l'import : les espaces
// extérieurs sont ignorés et la comparaison ne tient pas compte de la casse.
fn normalized_name_key(name: &str) -> Option<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() { None } else { Some(trimmed.to_lowercase()) }
}

// Normalise les URL Suno pour reconnaître le même lien malgré la casse,
// une barre finale, une query string ou un fragment.
fn normalized_suno_url(url: &str) -> Option<String> {
    let trimmed = url.trim();
    if trimmed.is_empty() { return None; }
    let without_suffix = trimmed.split(['?', '#']).next().unwrap_or(trimmed).trim_end_matches('/');
    if without_suffix.is_empty() { None } else { Some(without_suffix.to_lowercase()) }
}

fn is_duplicate_suno_entry(styles: &[SunoStyle], name: &str, url: &str) -> bool {
    let name_key = normalized_name_key(name);
    let url_key = normalized_suno_url(url);
    styles.iter().any(|style| {
        name_key.as_ref().is_some_and(|key| normalized_name_key(&style.name).as_ref() == Some(key))
            || url_key.as_ref().is_some_and(|key| normalized_suno_url(&style.suno_url).as_ref() == Some(key))
    })
}

// Les styles musicaux sont stockés sous forme de liste séparée par des virgules.
// Le filtre compare un tag entier, sans tenir compte de la casse ni des espaces périphériques.
fn music_style_has_tag(music_style: &str, wanted_tag: &str) -> bool {
    let wanted = wanted_tag.trim();
    !wanted.is_empty() && music_style.split(',').any(|tag| tag.trim().eq_ignore_ascii_case(wanted))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum EditorMode { New, Edit }

struct SunoApp {
    db: Connection,
    styles: Vec<SunoStyle>,
    search: String,
    search_open: bool,
    search_name: bool,
    search_music_style: bool,
    search_tags: bool,
    search_style: bool,
    search_exclusion: bool,
    search_author: bool,
    sort_by: String,
    favorites_only: bool,
    active_music_tag: Option<String>,
    selected_id: Option<i64>,
    editor_open: bool,
    editor_mode: EditorMode,
    draft: SunoStyle,
    song_url: String,
    suno_analysis_checked: bool,
    status: String,
    confirm_delete: bool,
    import_dialog_open: bool,
    import_report: Option<(usize, usize, bool)>,
    settings_open: bool,
    settings_about: bool,
    manual_open: bool,
    settings: AppSettings,
    system_visuals: Option<egui::Visuals>,
}

fn draw_icon(painter: &egui::Painter, center: egui::Pos2, icon: &str, color: egui::Color32) {
    use egui::{pos2, Stroke};
    let stroke = Stroke::new(2.0_f32, color);
    let x = center.x;
    let y = center.y;
    match icon {
        "new" => {
            painter.line_segment([pos2(x-8.0,y), pos2(x+8.0,y)], stroke);
            painter.line_segment([pos2(x,y-8.0), pos2(x,y+8.0)], stroke);
        }
        "edit" => {
            painter.line_segment([pos2(x-7.0,y+7.0), pos2(x+6.0,y-6.0)], Stroke::new(4.0_f32,color));
            painter.line_segment([pos2(x-9.0,y+9.0), pos2(x-5.0,y+8.0)], stroke);
            painter.line_segment([pos2(x+5.0,y-8.0), pos2(x+8.0,y-5.0)], stroke);
        }
        "delete" => {
            painter.line_segment([pos2(x-7.0,y-7.0),pos2(x+7.0,y+7.0)], stroke);
            painter.line_segment([pos2(x+7.0,y-7.0),pos2(x-7.0,y+7.0)], stroke);
        }
        "import" | "export" => {
            let down = icon == "import";
            let (top,bottom) = if down {(y-8.0,y+3.0)} else {(y+8.0,y-3.0)};
            painter.line_segment([pos2(x,top),pos2(x,bottom)], stroke);
            if down {
                painter.line_segment([pos2(x-6.0,y-1.0),pos2(x,y+5.0)], stroke);
                painter.line_segment([pos2(x+6.0,y-1.0),pos2(x,y+5.0)], stroke);
                painter.line_segment([pos2(x-9.0,y+8.0),pos2(x+9.0,y+8.0)], stroke);
            } else {
                painter.line_segment([pos2(x-6.0,y+1.0),pos2(x,y-5.0)], stroke);
                painter.line_segment([pos2(x+6.0,y+1.0),pos2(x,y-5.0)], stroke);
                painter.line_segment([pos2(x-9.0,y-8.0),pos2(x+9.0,y-8.0)], stroke);
            }
        }
        "search" => {
            painter.circle_stroke(pos2(x-3.0,y-3.0),6.0,stroke);
            painter.line_segment([pos2(x+2.0,y+2.0),pos2(x+9.0,y+9.0)], stroke);
        }
        "filter" => {
            painter.line_segment([pos2(x-10.0,y-7.0),pos2(x+10.0,y-7.0)], stroke);
            painter.line_segment([pos2(x-7.0,y-1.0),pos2(x+7.0,y-1.0)], stroke);
            painter.line_segment([pos2(x-3.0,y+5.0),pos2(x+3.0,y+5.0)], stroke);
        }
        "library" => {
            // Icône de bibliothèque : trois lignes et repères, dessinée pour rester lisible
            // indépendamment des polices installées sur le système.
            painter.rect_stroke(egui::Rect::from_min_max(pos2(x-9.0,y-9.0), pos2(x+9.0,y+9.0)), 2.0, stroke, egui::StrokeKind::Outside);
            for offset in [-4.0_f32, 1.0, 6.0] {
                painter.circle_filled(pos2(x-5.0,y+offset-1.0), 1.0, color);
                painter.line_segment([pos2(x-1.0,y+offset-1.0), pos2(x+6.0,y+offset-1.0)], stroke);
            }
        }
        "favorite" => {
            let mut points = Vec::with_capacity(10);
            for i in 0..10 {
                let angle = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / 5.0;
                let radius = if i % 2 == 0 { 8.5 } else { 3.8 };
                points.push(pos2(x + angle.cos() * radius, y + angle.sin() * radius));
            }
            for i in 0..10 {
                painter.line_segment([points[i], points[(i + 1) % 10]], stroke);
            }
        }
        "manual" | "help" => {
            // Livre ouvert : deux pages symétriques et une reliure centrale.
            painter.line_segment([pos2(x,y-7.0), pos2(x,y+8.0)], stroke);
            painter.line_segment([pos2(x,y-6.0), pos2(x-5.0,y-8.0)], stroke);
            painter.line_segment([pos2(x-5.0,y-8.0), pos2(x-10.0,y-6.0)], stroke);
            painter.line_segment([pos2(x-10.0,y-6.0), pos2(x-10.0,y+6.0)], stroke);
            painter.line_segment([pos2(x-10.0,y+6.0), pos2(x-5.0,y+8.0)], stroke);
            painter.line_segment([pos2(x-5.0,y+8.0), pos2(x,y+8.0)], stroke);
            painter.line_segment([pos2(x,y-6.0), pos2(x+5.0,y-8.0)], stroke);
            painter.line_segment([pos2(x+5.0,y-8.0), pos2(x+10.0,y-6.0)], stroke);
            painter.line_segment([pos2(x+10.0,y-6.0), pos2(x+10.0,y+6.0)], stroke);
            painter.line_segment([pos2(x+10.0,y+6.0), pos2(x+5.0,y+8.0)], stroke);
            painter.line_segment([pos2(x+5.0,y+8.0), pos2(x,y+8.0)], stroke);
            painter.line_segment([pos2(x-7.0,y-2.0), pos2(x-3.0,y-1.0)], stroke);
            painter.line_segment([pos2(x+3.0,y-1.0), pos2(x+7.0,y-2.0)], stroke);
        }
        "ready" => {
            painter.circle_stroke(center, 8.0, stroke);
            painter.line_segment([pos2(x-4.0,y), pos2(x-1.0,y+3.0)], stroke);
            painter.line_segment([pos2(x-1.0,y+3.0), pos2(x+5.0,y-4.0)], stroke);
        }
        "settings" => {
            painter.circle_stroke(center,8.0,stroke);
            painter.circle_filled(center,3.0,color);
            for a in 0..8 {
                let angle = a as f32 * std::f32::consts::TAU / 8.0;
                let inner = pos2(x+angle.cos()*8.0,y+angle.sin()*8.0);
                let outer = pos2(x+angle.cos()*11.0,y+angle.sin()*11.0);
                painter.line_segment([inner,outer], Stroke::new(2.5_f32,color));
            }
        }
        _ => { painter.circle_stroke(center,7.0,stroke); }
    }
}

fn icon_button(ui: &mut egui::Ui, icon: &str, label: &str, width: f32) -> egui::Response {
    icon_button_enabled(ui, icon, label, width, true)
}

fn icon_button_enabled(ui: &mut egui::Ui, icon: &str, label: &str, width: f32, enabled: bool) -> egui::Response {
    let sense = if enabled { egui::Sense::click() } else { egui::Sense::hover() };
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 62.0), sense);
    let visuals = ui.style().interact(&response);
    let disabled_fill = ui.visuals().faint_bg_color;
    let fill = if !enabled { disabled_fill } else if response.hovered() { visuals.bg_fill } else { ui.visuals().panel_fill };
    let stroke_color = if enabled { ui.visuals().widgets.noninteractive.bg_stroke.color } else { ui.visuals().weak_text_color() };
    ui.painter().rect_filled(rect, 7.0, fill);
    ui.painter().rect_stroke(rect, 7.0, egui::Stroke::new(1.0_f32, stroke_color), egui::StrokeKind::Outside);
    let icon_color = if enabled { if response.hovered() { ui.visuals().strong_text_color() } else { ui.visuals().text_color() } } else { ui.visuals().weak_text_color() };
    draw_icon(ui.painter(), egui::pos2(rect.center().x, rect.top()+19.0), icon, icon_color);
    ui.painter().text(egui::pos2(rect.center().x, rect.top()+45.0), egui::Align2::CENTER_CENTER, label, egui::FontId::proportional(12.5), icon_color);
    response
}

#[derive(Clone, Copy)]
enum ModalButtonTone {
    Primary,
    Secondary,
    Danger,
}

/// Boutons d'action des fenêtres : couleurs explicites, coins arrondis et état de survol visible.
fn modal_button(ui: &mut egui::Ui, label: &str, tone: ModalButtonTone) -> egui::Response {
    let width = ((label.chars().count() as f32) * 7.4 + 34.0).clamp(96.0, 220.0);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 34.0), egui::Sense::click());
    let dark = ui.visuals().dark_mode;
    let (normal, hover, text_color, border) = match tone {
        ModalButtonTone::Primary => (
            egui::Color32::from_rgb(43, 111, 190),
            egui::Color32::from_rgb(55, 132, 218),
            egui::Color32::WHITE,
            egui::Color32::from_rgb(65, 140, 220),
        ),
        ModalButtonTone::Secondary => (
            if dark { egui::Color32::from_rgb(55, 65, 80) } else { egui::Color32::from_rgb(235, 240, 247) },
            if dark { egui::Color32::from_rgb(70, 83, 101) } else { egui::Color32::from_rgb(220, 231, 244) },
            ui.visuals().text_color(),
            if dark { egui::Color32::from_rgb(86, 99, 117) } else { egui::Color32::from_rgb(202, 214, 229) },
        ),
        ModalButtonTone::Danger => (
            egui::Color32::from_rgb(177, 54, 59),
            egui::Color32::from_rgb(205, 65, 70),
            egui::Color32::WHITE,
            egui::Color32::from_rgb(205, 75, 80),
        ),
    };
    let fill = if response.hovered() { hover } else { normal };
    ui.painter().rect_filled(rect, 8.0, fill);
    ui.painter().rect_stroke(rect, 8.0, egui::Stroke::new(1.0_f32, border), egui::StrokeKind::Outside);
    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, label, egui::FontId::proportional(13.5), text_color);
    response
}

fn draw_flag(ui: &mut egui::Ui, language: &str) {
    use egui::{pos2, vec2, Color32, Rect};
    let (rect, _) = ui.allocate_exact_size(vec2(24.0, 16.0), egui::Sense::hover());
    let p = ui.painter();
    let white = Color32::WHITE;
    let red = Color32::from_rgb(210, 35, 45);
    let blue = Color32::from_rgb(25, 55, 125);
    let green = Color32::from_rgb(0, 120, 65);
    // egui 0.31 requires both a corner radius and a fill color.
    let fill = |r: Rect, color: Color32| p.rect_filled(r, 0.0, color);
    match language {
        "Français" => {
            fill(Rect::from_min_max(rect.min, pos2(rect.left() + 8.0, rect.bottom())), blue);
            fill(Rect::from_min_max(pos2(rect.left() + 8.0, rect.top()), pos2(rect.left() + 16.0, rect.bottom())), white);
            fill(Rect::from_min_max(pos2(rect.left() + 16.0, rect.top()), rect.max), red);
        }
        "English" => {
            fill(rect, blue);
            fill(Rect::from_min_max(pos2(rect.left(), rect.center().y - 2.5), pos2(rect.right(), rect.center().y + 2.5)), white);
            fill(Rect::from_min_max(pos2(rect.center().x - 2.5, rect.top()), pos2(rect.center().x + 2.5, rect.bottom())), white);
            fill(Rect::from_min_max(pos2(rect.left(), rect.center().y - 1.0), pos2(rect.right(), rect.center().y + 1.0)), red);
            fill(Rect::from_min_max(pos2(rect.center().x - 1.0, rect.top()), pos2(rect.center().x + 1.0, rect.bottom())), red);
        }
        "Deutsch" => {
            fill(Rect::from_min_max(rect.min, pos2(rect.right(), rect.top() + 5.33)), Color32::BLACK);
            fill(Rect::from_min_max(pos2(rect.left(), rect.top() + 5.33), pos2(rect.right(), rect.top() + 10.66)), red);
            fill(Rect::from_min_max(pos2(rect.left(), rect.top() + 10.66), rect.max), Color32::from_rgb(255, 195, 0));
        }
        "Español" => {
            fill(rect, Color32::from_rgb(190, 20, 35));
            fill(Rect::from_min_max(pos2(rect.left(), rect.top() + 4.0), pos2(rect.right(), rect.bottom() - 4.0)), Color32::from_rgb(255, 195, 0));
        }
        "Português" => {
            fill(Rect::from_min_max(rect.min, pos2(rect.left() + 9.0, rect.bottom())), green);
            fill(Rect::from_min_max(pos2(rect.left() + 9.0, rect.top()), rect.max), red);
            p.circle_filled(rect.center(), 3.0, Color32::from_rgb(255, 210, 0));
        }
        "中文" => {
            fill(rect, red);
            p.circle_filled(pos2(rect.left() + 6.0, rect.top() + 5.0), 2.0, Color32::from_rgb(255, 220, 0));
            p.circle_filled(pos2(rect.left() + 11.0, rect.top() + 3.0), 1.0, Color32::from_rgb(255, 220, 0));
            p.circle_filled(pos2(rect.left() + 12.0, rect.top() + 7.0), 1.0, Color32::from_rgb(255, 220, 0));
        }
        "العربية" => {
            fill(rect, green);
            p.line_segment([pos2(rect.left() + 4.0, rect.center().y + 2.0), pos2(rect.right() - 4.0, rect.center().y + 2.0)], egui::Stroke::new(1.5_f32, white));
        }
        _ => {
            fill(rect, white);
        }
    }
    p.rect_stroke(rect, 1.0, egui::Stroke::new(0.5_f32, ui.visuals().widgets.noninteractive.bg_stroke.color), egui::StrokeKind::Outside);
}

#[derive(Default)]
struct ParsedSunoSong {
    title: String,
    author: String,
    tags: String,
    style: String,
    exclusion: String,
}

fn html_unescape(input: &str) -> String {
    input.replace("&amp;", "&").replace("&quot;", "\"").replace("&#39;", "'")
        .replace("&apos;", "'").replace("&lt;", "<").replace("&gt;", ">")
        .replace("&#x27;", "'").replace("&#34;", "\"")
}

fn meta_content(html: &str, wanted: &str) -> Option<String> {
    for part in html.split('<') {
        let tag = part.split('>').next().unwrap_or("");
        if !tag.to_ascii_lowercase().starts_with("meta ") && !tag.eq_ignore_ascii_case("meta") { continue; }
        let lower = tag.to_ascii_lowercase();
        let is_match = [format!("property=\"{wanted}\""), format!("property='{wanted}'"), format!("name=\"{wanted}\""), format!("name='{wanted}'")]
            .iter().any(|needle| lower.contains(needle));
        if !is_match { continue; }
        if let Some(pos) = lower.find("content=") {
            let rest = &tag[pos + 8..];
            let quote = rest.chars().next()?;
            if quote == '\"' || quote == '\'' {
                if let Some(end) = rest[1..].find(quote) { return Some(html_unescape(&rest[1..1 + end])); }
            }
            return Some(rest.split_whitespace().next().unwrap_or("").trim_matches(|c| c == '\"' || c == '\'').to_string());
        }
    }
    None
}

fn json_string(value: &Value) -> Option<String> {
    match value {
        Value::String(s) if !s.trim().is_empty() => Some(s.trim().to_string()),
        Value::Array(items) => {
            let values: Vec<String> = items.iter().filter_map(json_string).collect();
            if values.is_empty() { None } else { Some(values.join(", ")) }
        }
        _ => None,
    }
}

fn normalize_key(key: &str) -> String {
    key.to_ascii_lowercase().replace('_', "").replace('-', "").replace(' ', "")
}

fn collect_json_fields(value: &Value, keys: &[&str], out: &mut Vec<String>) {
    match value {
        Value::Object(map) => for (key, child) in map {
            let normalized = normalize_key(key);
            if keys.iter().any(|k| normalized == normalize_key(k)) {
                if let Some(s) = json_string(child) { out.push(s); }
            }
            collect_json_fields(child, keys, out);
        },
        Value::Array(items) => for child in items { collect_json_fields(child, keys, out); },
        _ => {},
    }
}

// Suno peut stocker le prompt de style complet dans `tags`, avec les exclusions
// à la fin sous la forme « ‑public, ‑live, … ». On les sépare pour remplir
// les deux champs de l'application sans perdre le prompt musical.
fn split_embedded_exclusions(input: &str) -> (String, String) {
    let mut style_parts = Vec::new();
    let mut exclusion_parts = Vec::new();
    for part in input.split(',') {
        let trimmed = part.trim();
        let mut chars = trimmed.chars();
        let first = chars.next();
        let is_negative = matches!(first, Some('-' | '‐' | '‑' | '‒' | '–' | '—' | '−'));
        let remainder = if is_negative { chars.as_str().trim() } else { trimmed };
        if is_negative && !remainder.is_empty() {
            exclusion_parts.push(remainder.to_string());
        } else if !trimmed.is_empty() {
            style_parts.push(trimmed.to_string());
        }
    }
    (style_parts.join(", "), exclusion_parts.join(", "))
}

fn best_nonempty_longest(values: Vec<String>) -> Option<String> {
    values.into_iter()
        .filter(|s| !s.trim().is_empty())
        .max_by_key(|s| s.trim().len())
        .map(|s| s.trim().to_string())
}

// Suno peut intégrer son état applicatif dans une chaîne JSON échappée.
fn parse_balanced_json_at(text: &str, start: usize) -> Option<(Value, usize)> {
    let bytes = text.as_bytes();
    if start >= bytes.len() || (bytes[start] != b'{' && bytes[start] != b'[') { return None; }
    let mut stack: Vec<u8> = Vec::new();
    let mut in_string = false;
    let mut escaped = false;
    for (offset, &byte) in bytes[start..].iter().enumerate() {
        if in_string {
            if escaped { escaped = false; }
            else if byte == b'\\' { escaped = true; }
            else if byte == b'"' { in_string = false; }
            continue;
        }
        if byte == b'"' { in_string = true; continue; }
        if byte == b'{' { stack.push(b'}'); }
        else if byte == b'[' { stack.push(b']'); }
        else if byte == b'}' || byte == b']' {
            if stack.last().copied() != Some(byte) { return None; }
            stack.pop();
            if stack.is_empty() {
                let end = start + offset + 1;
                let parsed = serde_json::from_slice::<Value>(&bytes[start..end]).ok()?;
                return Some((parsed, end));
            }
        }
    }
    None
}

fn collect_nested_json_strings(value: &Value, roots: &mut Vec<Value>, depth: usize) {
    if depth > 5 { return; }
    match value {
        Value::String(s) if s.len() < 8_000_000 => {
            if let Ok(parsed) = serde_json::from_str::<Value>(s) {
                roots.push(parsed.clone());
                collect_nested_json_strings(&parsed, roots, depth + 1);
                return;
            }
            let mut i = 0;
            while i < s.len() {
                let byte = s.as_bytes()[i];
                if byte != b'{' && byte != b'[' { i += 1; continue; }
                if let Some((parsed, end)) = parse_balanced_json_at(s, i) {
                    roots.push(parsed.clone());
                    collect_nested_json_strings(&parsed, roots, depth + 1);
                    i = end;
                } else { i += 1; }
            }
        }
        Value::Object(map) => for child in map.values() { collect_nested_json_strings(child, roots, depth + 1); },
        Value::Array(items) => for child in items { collect_nested_json_strings(child, roots, depth + 1); },
        _ => {},
    }
}

// Extrait le texte visible des liens de profil Suno (href="/\@pseudo" ou
// "https://suno.com/\@pseudo"). Cela reste fiable même si le nom du créateur
// n'est pas présent dans les objets JSON sérialisés de la page.
fn collect_profile_author_from_html(html: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let mut from = 0usize;
    while let Some(pos) = lower[from..].find("<a") {
        let a_start = from + pos;
        let open_end = lower[a_start..].find('>').map(|n| a_start + n)?;
        let opening = &lower[a_start..=open_end];
        let is_profile = opening.contains("suno.com/@")
            || opening.contains(r#"href="/@"#) || opening.contains("href='/@");
        if is_profile {
            if let Some(close_offset) = lower[open_end + 1..].find("</a>") {
                let close = open_end + 1 + close_offset;
                let inner = &html[open_end + 1..close];
                let mut text = String::new();
                let mut in_tag = false;
                for ch in inner.chars() {
                    if ch == '<' { in_tag = true; }
                    else if ch == '>' { in_tag = false; text.push(' '); }
                    else if !in_tag { text.push(ch); }
                }
                let name = html_unescape(&text).split_whitespace().collect::<Vec<_>>().join(" ");
                if !name.is_empty() && !name.starts_with('@') && name.len() < 100 {
                    return Some(name);
                }
            }
        }
        from = open_end + 1;
        if from >= html.len() { break; }
    }
    None
}

// Le HTML brut contient les styles CSS de l'application, qui peuvent avoir des
// crochets ressemblant à des tags de paroles. On retire scripts/styles avant de
// tenter une extraction de secours à partir du texte réellement affiché.
fn visible_html_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len().min(1_000_000));
    let lower = html.to_ascii_lowercase();
    let mut pos = 0usize;
    while pos < html.len() {
        let next_script = lower[pos..].find("<script").map(|n| pos + n);
        let next_style = lower[pos..].find("<style").map(|n| pos + n);
        let next = match (next_script, next_style) {
            (Some(a), Some(b)) => a.min(b), (Some(a), None) => a,
            (None, Some(b)) => b, (None, None) => html.len(),
        };
        if next > pos {
            let segment = &html[pos..next];
            let mut in_tag = false;
            for ch in segment.chars() {
                if ch == '<' { in_tag = true; }
                else if ch == '>' { in_tag = false; out.push(' '); }
                else if !in_tag { out.push(ch); }
            }
        }
        if next == html.len() { break; }
        let is_script = lower[next..].starts_with("<script");
        let closing = if is_script { "</script>" } else { "</style>" };
        if let Some(end) = lower[next..].find(closing) {
            pos = next + end + closing.len();
        } else { break; }
    }
    html_unescape(&out)
}

fn collect_author_fields(value: &Value, in_author_context: bool, out: &mut Vec<String>) {
    match value {
        Value::Object(map) => for (key, child) in map {
            let k = key.to_ascii_lowercase().replace('_', "").replace('-', "");
            let author_context = in_author_context || ["user", "creator", "owner", "artist", "profile", "author"].contains(&k.as_str());
            if author_context && ["handle", "username", "displayname", "name"].contains(&k.as_str()) {
                if let Some(s) = json_string(child) { out.push(s); }
            }
            collect_author_fields(child, author_context, out);
        },
        Value::Array(items) => for child in items { collect_author_fields(child, in_author_context, out); },
        _ => {}
    }
}

fn extract_bracket_tags(lyrics: &str) -> String {
    let ignored = ["chorus", "verse", "intro", "bridge", "outro", "couplet", "refrain", "pont", "coda"];
    let mut tags = Vec::<String>::new();
    let mut rest = lyrics;
    while let Some(start) = rest.find('[') {
        let after = &rest[start + 1..];
        let Some(end) = after.find(']') else { break; };
        let tag = after[..end].trim();
        let lower = tag.to_lowercase();
        let looks_like_markup = tag.len() > 220 || tag.chars().any(|c| "<>{}\"".contains(c));
        let only_json_separators = tag.chars().all(|c| c.is_ascii_digit() || c.is_ascii_whitespace() || [',', ':', '-', '.'].contains(&c));
        if !tag.is_empty() && !looks_like_markup && !only_json_separators
            && !ignored.iter().any(|word| lower.contains(word))
            && !tags.iter().any(|old| old.eq_ignore_ascii_case(tag)) {
            tags.push(tag.to_string());
        }
        rest = &after[end + 1..];
    }
    tags.join(", ")
}

fn parse_suno_song(url: &str) -> Result<ParsedSunoSong, String> {
    let trimmed = url.trim();
    if !(trimmed.starts_with("https://suno.com/") || trimmed.starts_with("https://www.suno.com/")) {
        return Err("Colle un lien public https://suno.com/... vers une chanson Suno.".into());
    }
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(15))
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/124.0 Safari/537.36")
        .build().map_err(|e| format!("Impossible de préparer la requête : {e}"))?;
    let response = client.get(trimmed).send().map_err(|e| format!("Impossible de récupérer la page Suno : {e}"))?;
    if !response.status().is_success() { return Err(format!("Suno a répondu avec le statut HTTP {}.", response.status())); }
    let html = response.text().map_err(|e| format!("Impossible de lire la page Suno : {e}"))?;

    let mut json_roots = Vec::<Value>::new();
    let mut cursor = html.as_str();
    while let Some(start) = cursor.to_ascii_lowercase().find("<script") {
        cursor = &cursor[start..];
        let Some(open_end) = cursor.find('>') else { break; };
        let body_start = open_end + 1;
        let lower_body = cursor[body_start..].to_ascii_lowercase();
        let Some(close) = lower_body.find("</script>") else { break; };
        let body = cursor[body_start..body_start + close].trim();
        if body.starts_with('{') || body.starts_with('[') {
            if let Ok(value) = serde_json::from_str::<Value>(body) {
                json_roots.push(value.clone());
                collect_nested_json_strings(&value, &mut json_roots, 0);
            }
        } else {
            // Next.js sérialise parfois l'état dans self.__next_f.push([1, "…"]).
            // On parse d'abord le tableau d'argument afin de décoder ses chaînes échappées.
            let mut search_from = 0;
            while let Some(push_pos) = body[search_from..].find("push(") {
                let after_push = search_from + push_pos + 5;
                if let Some(array_offset) = body[after_push..].find('[') {
                    let array_start = after_push + array_offset;
                    if let Some((value, _)) = parse_balanced_json_at(body, array_start) {
                        json_roots.push(value.clone());
                        collect_nested_json_strings(&value, &mut json_roots, 0);
                    }
                }
                search_from = after_push;
                if search_from >= body.len() { break; }
            }
            // Ne pas scanner brut le script JavaScript : ses crochets de code ne sont pas des données.
        }
        cursor = &cursor[body_start + close + 9..];
    }

    let mut title = meta_content(&html, "og:title").or_else(|| meta_content(&html, "twitter:title")).unwrap_or_default();
    let mut author_values = Vec::new();
    let mut lyric_values = Vec::new();
    let mut exclusion_values = Vec::new();
    let mut user_style_values = Vec::new();
    let mut style_prompt_values = Vec::new();
    let mut style_values = Vec::new();
    let mut prompt_values = Vec::new();
    for root in &json_roots {
        collect_author_fields(root, false, &mut author_values);
        collect_json_fields(root, &["lyrics", "lyric", "lyricstext", "lyriccontent"], &mut lyric_values);
        collect_json_fields(root, &["negativeTags", "exclude", "excludes", "excludestyles", "excludedstyles", "negativeprompt", "negativeprompttext"], &mut exclusion_values);
        collect_json_fields(root, &["user_style", "userstyle", "input_style"], &mut user_style_values);
        collect_json_fields(root, &["styleprompt", "musicprompt", "generationprompt"], &mut style_prompt_values);
        collect_json_fields(root, &["prompt"], &mut prompt_values);
        // Sur les pages Suno publiques, le champ `tags` peut contenir le prompt
        // descriptif entier, et pas uniquement une liste courte de genres.
        collect_json_fields(root, &["style", "styletext", "style_tags", "styletags", "tags"], &mut style_values);
    }
    // Titre depuis les données structurées si la balise Open Graph est absente.
    if title.is_empty() {
        for root in &json_roots {
            let mut candidates = Vec::new();
            collect_json_fields(root, &["title", "displayname", "songtitle", "tracktitle"], &mut candidates);
            if let Some(candidate) = candidates.into_iter().find(|s| !s.trim().is_empty()) { title = candidate; break; }
        }
    }
    if title.is_empty() { return Err("Le titre de la chanson n'a pas été trouvé dans la page publique.".into()); }
    let title_before_cleanup = title.clone();
    let profile_author = collect_profile_author_from_html(&html);
    let author = profile_author.or_else(|| author_values.into_iter().find(|s| !s.trim().is_empty())).or_else(|| {
        title_before_cleanup.split(" by ").nth(1).map(|s| s.split(" | Suno").next().unwrap_or(s).trim().to_string())
    }).unwrap_or_default();
    let mut title = title.split(" | Suno").next().unwrap_or(&title).trim().to_string();
    if !author.is_empty() {
        let suffix = format!(" by {author}");
        if title.ends_with(&suffix) { title.truncate(title.len() - suffix.len()); title = title.trim().to_string(); }
    }
    // Priorité au champ Lyrics structuré (prendre la version la plus complète).
    // En secours, analyser uniquement le texte visible du HTML, jamais le CSS brut.
    let lyrics = lyric_values.into_iter()
        .filter(|s| s.contains('[') && s.contains(']'))
        .max_by_key(|s| s.len())
        .unwrap_or_else(|| visible_html_text(&html));
    let tags = extract_bracket_tags(&lyrics);
    // Choisir la valeur la plus complète : certaines propriétés `user_style`
    // ne contiennent que les genres, tandis que `tags` porte le prompt détaillé.
    let mut all_style_candidates = user_style_values;
    all_style_candidates.extend(style_prompt_values);
    all_style_candidates.extend(style_values);
    let raw_style = best_nonempty_longest(all_style_candidates)
        .or_else(|| best_nonempty_longest(prompt_values))
        .unwrap_or_default();
    let (style_without_embedded_exclusions, embedded_exclusion) = split_embedded_exclusions(&raw_style);
    let style = if style_without_embedded_exclusions.trim().is_empty() { raw_style } else { style_without_embedded_exclusions };
    let exclusion = best_nonempty_longest(exclusion_values)
        .filter(|s| !s.trim().is_empty())
        .or_else(|| if embedded_exclusion.is_empty() { None } else { Some(embedded_exclusion) })
        .unwrap_or_default();
    Ok(ParsedSunoSong { title, author, tags, style, exclusion })
}

impl SunoApp {
    fn language_label(language: &str) -> &'static str {
        match language {
            "English" => "Language",
            "Deutsch" => "Sprache",
            "Español" => "Idioma",
            "Português" => "Idioma",
            "中文" => "语言",
            "العربية" => "اللغة",
            _ => "Langue",
        }
    }

    fn tr<'a>(language: &str, key: &'a str) -> &'a str {
        match key {
            "manual" => match language {
                "Français" => "Manuel", "English" => "Manual", "Deutsch" => "Handbuch",
                "Español" => "Manual", "Português" => "Manual", "中文" => "使用手册", "العربية" => "الدليل", _ => "Manuel",
            },
            "manual_title" => match language {
                "Français" => "Manuel d’utilisation", "English" => "User manual", "Deutsch" => "Benutzerhandbuch",
                "Español" => "Manual de usuario", "Português" => "Manual do utilizador", "中文" => "使用手册", "العربية" => "دليل الاستخدام", _ => "Manuel d’utilisation",
            },
            "manual_intro" => match language {
                "Français" => "Retrouvez ici les principales fonctions de Suno Style Manager.",
                "English" => "A quick guide to the main features of Suno Style Manager.",
                "Deutsch" => "Hier finden Sie eine kurze Anleitung zu den wichtigsten Funktionen von Suno Style Manager.",
                "Español" => "Aquí encontrarás una guía rápida de las funciones principales de Suno Style Manager.",
                "Português" => "Aqui encontra um guia rápido das principais funções do Suno Style Manager.",
                "中文" => "这里介绍 Suno Style Manager 的主要功能。", "العربية" => "دليل سريع لأهم وظائف Suno Style Manager.", _ => "Retrouvez ici les principales fonctions de Suno Style Manager.",
            },
            "manual_tooltip" => match language {
                "Français" => "Ouvrir le guide d’utilisation", "English" => "Open the user guide", "Deutsch" => "Benutzerhandbuch öffnen",
                "Español" => "Abrir la guía de usuario", "Português" => "Abrir o manual do utilizador", "中文" => "打开使用指南", "العربية" => "فتح دليل الاستخدام", _ => "Ouvrir le guide d’utilisation",
            },
            "add_suno_link" => match language {
                "Français" => "Ajouter le lien Suno",
                "English" => "Save Suno link",
                "Deutsch" => "Suno-Link speichern",
                "Español" => "Guardar enlace de Suno",
                "Português" => "Guardar link do Suno",
                "中文" => "保存 Suno 链接",
                "العربية" => "حفظ رابط Suno",
                _ => "Ajouter le lien Suno",
            },
            "delete_suno_links" => match language {
                "Français" => "Supprimer les liens Suno",
                "English" => "Delete Suno links",
                "Deutsch" => "Suno-Links löschen",
                "Español" => "Eliminar enlaces de Suno",
                "Português" => "Eliminar links do Suno",
                "中文" => "删除 Suno 链接",
                "العربية" => "حذف روابط Suno",
                _ => "Supprimer les liens Suno",
            },
            "suno_links_deleted" => match language {
                "Français" => "Tous les liens Suno ont été supprimés de la bibliothèque.",
                "English" => "All Suno links were removed from the library.",
                "Deutsch" => "Alle Suno-Links wurden aus der Bibliothek entfernt.",
                "Español" => "Se eliminaron todos los enlaces de Suno de la biblioteca.",
                "Português" => "Todos os links do Suno foram removidos da biblioteca.",
                "中文" => "已从资料库中删除所有 Suno 链接。",
                "العربية" => "تم حذف جميع روابط Suno من المكتبة.",
                _ => "Tous les liens Suno ont été supprimés de la bibliothèque.",
            },
            "about" => match language {
                "Français" => "À propos", "English" => "About", "Deutsch" => "Über", "Español" => "Acerca de",
                "Português" => "Sobre", "中文" => "关于", "العربية" => "حول", _ => "À propos",
            },
            "back_to_options" => match language {
                "Français" => "Retour aux options", "English" => "Back to settings", "Deutsch" => "Zurück zu den Optionen",
                "Español" => "Volver a opciones", "Português" => "Voltar às opções", "中文" => "返回设置", "العربية" => "العودة إلى الإعدادات", _ => "Retour aux options",
            },
            "creator" => match language {
                "Français" => "Créateur : L'Homme de Paille", "English" => "Created by: L'Homme de Paille",
                "Deutsch" => "Erstellt von: L'Homme de Paille", "Español" => "Creado por: L'Homme de Paille",
                "Português" => "Criado por: L'Homme de Paille", "中文" => "创作者：L'Homme de Paille", "العربية" => "إنشاء: L'Homme de Paille", _ => "Créateur : L'Homme de Paille",
            },
            "app_version" => match language {
                "Français" => "Version 1.7.0", "English" => "Version 1.7.0", "Deutsch" => "Version 1.7.0",
                "Español" => "Versión 1.7.0", "Português" => "Versão 1.7.0", "中文" => "版本 1.7.0", "العربية" => "الإصدار 1.7.0", _ => "Version 1.7.0",
            },
            "close" => match language {
                "Français" => "Fermer", "English" => "Close", "Deutsch" => "Schließen", "Español" => "Cerrar",
                "Português" => "Fechar", "中文" => "关闭", "العربية" => "إغلاق", _ => "Fermer",
            },
            "new" => match language {
                "Français" => "Nouveau",
                "English" => "New",
                "Deutsch" => "Neu",
                "Español" => "Nuevo",
                "Português" => "Novo",
                "中文" => "新建",
                "العربية" => "جديد",
                _ => "Nouveau",
            },
            "edit" => match language {
                "Français" => "Édition",
                "English" => "Edit",
                "Deutsch" => "Bearbeiten",
                "Español" => "Editar",
                "Português" => "Editar",
                "中文" => "编辑",
                "العربية" => "تعديل",
                _ => "Édition",
            },
            "delete" => match language {
                "Français" => "Supprimer",
                "English" => "Delete",
                "Deutsch" => "Löschen",
                "Español" => "Eliminar",
                "Português" => "Excluir",
                "中文" => "删除",
                "العربية" => "حذف",
                _ => "Supprimer",
            },
            "import" => match language {
                "Français" => "Importer",
                "English" => "Import",
                "Deutsch" => "Importieren",
                "Español" => "Importar",
                "Português" => "Importar",
                "中文" => "导入",
                "العربية" => "استيراد",
                _ => "Importer",
            },
            "export" => match language {
                "Français" => "Exporter",
                "English" => "Export",
                "Deutsch" => "Exportieren",
                "Español" => "Exportar",
                "Português" => "Exportar",
                "中文" => "导出",
                "العربية" => "تصدير",
                _ => "Exporter",
            },
            "search" => match language {
                "Français" => "Rechercher",
                "English" => "Search",
                "Deutsch" => "Suchen",
                "Español" => "Buscar",
                "Português" => "Pesquisar",
                "中文" => "搜索",
                "العربية" => "بحث",
                _ => "Rechercher",
            },
            "clear" => match language {
                "Français" => "Effacer",
                "English" => "Clear",
                "Deutsch" => "Leeren",
                "Español" => "Limpiar",
                "Português" => "Limpar",
                "中文" => "清除",
                "العربية" => "مسح",
                _ => "Effacer",
            },
            "search_in" => match language {
                "Français" => "Rechercher dans les colonnes :",
                "English" => "Search columns:",
                "Deutsch" => "Spalten durchsuchen:",
                "Español" => "Buscar en las columnas:",
                "Português" => "Pesquisar nas colunas:",
                "中文" => "搜索列：",
                "العربية" => "البحث في الأعمدة:",
                _ => "Rechercher dans les colonnes :",
            },
            "name" => match language {
                "Français" => "Nom du style",
                "English" => "Style name",
                "Deutsch" => "Stilname",
                "Español" => "Nombre del estilo",
                "Português" => "Nome do estilo",
                "中文" => "风格名称",
                "العربية" => "اسم النمط",
                _ => "Nom du style",
            },
            "tags" => match language {
                "Français" => "Tags",
                "English" => "Tags",
                "Deutsch" => "Tags",
                "Español" => "Etiquetas",
                "Português" => "Tags",
                "中文" => "标签",
                "العربية" => "الوسوم",
                _ => "Tags",
            },
            "music_style" => match language {
                "Français" => "Style musical",
                "English" => "Musical style",
                "Deutsch" => "Musikstil",
                "Español" => "Estilo musical",
                "Português" => "Estilo musical",
                "中文" => "音乐风格",
                "العربية" => "النمط الموسيقي",
                _ => "Style musical",
            },
            "lyrics_tags" => match language {
                "Français" => "Tags de la zone lyrics",
                "English" => "Lyrics section tags",
                "Deutsch" => "Lyrics-Bereich-Tags",
                "Español" => "Etiquetas de la zona lyrics",
                "Português" => "Tags da área lyrics",
                "中文" => "歌词区标签",
                "العربية" => "وسوم قسم الكلمات",
                _ => "Tags de la zone lyrics",
            },
            "style" => match language {
                "Français" => "Style",
                "English" => "Style",
                "Deutsch" => "Stil",
                "Español" => "Estilo",
                "Português" => "Estilo",
                "中文" => "风格",
                "العربية" => "النمط",
                _ => "Style",
            },
            "exclusion" => match language {
                "Français" => "Exclusion",
                "English" => "Exclusions",
                "Deutsch" => "Ausschlüsse",
                "Español" => "Exclusiones",
                "Português" => "Exclusões",
                "中文" => "排除项",
                "العربية" => "الاستثناءات",
                _ => "Exclusion",
            },
            "all" => match language {
                "Français" => "Tous les styles",
                "English" => "All styles",
                "Deutsch" => "Alle Stile",
                "Español" => "Todos los estilos",
                "Português" => "Todos os estilos",
                "中文" => "所有风格",
                "العربية" => "كل الأنماط",
                _ => "Tous les styles",
            },
            "favorites" => match language {
                "Français" => "Favoris",
                "English" => "Favorites",
                "Deutsch" => "Favoriten",
                "Español" => "Favoritos",
                "Português" => "Favoritos",
                "中文" => "收藏",
                "العربية" => "المفضلة",
                _ => "Favoris",
            },
            "options" => match language {
                "Français" => "Options",
                "English" => "Settings",
                "Deutsch" => "Optionen",
                "Español" => "Opciones",
                "Português" => "Opções",
                "中文" => "选项",
                "العربية" => "الإعدادات",
                _ => "Options",
            },
            "theme" => match language {
                "Français" => "Mode d’affichage",
                "English" => "Display mode",
                "Deutsch" => "Anzeigemodus",
                "Español" => "Modo de visualización",
                "Português" => "Modo de exibição",
                "中文" => "显示模式",
                "العربية" => "وضع العرض",
                _ => "Mode d’affichage",
            },
            "save" => match language {
                "Français" => "Enregistrer",
                "English" => "Save",
                "Deutsch" => "Speichern",
                "Español" => "Guardar",
                "Português" => "Guardar",
                "中文" => "保存",
                "العربية" => "حفظ",
                _ => "Enregistrer",
            },
            "ok" => match language {
                "Français" => "OK",
                "English" => "OK",
                "Deutsch" => "OK",
                "Español" => "Aceptar",
                "Português" => "OK",
                "中文" => "确定",
                "العربية" => "حسنًا",
                _ => "OK",
            },
            "cancel" => match language {
                "Français" => "Annuler",
                "English" => "Cancel",
                "Deutsch" => "Abbrechen",
                "Español" => "Cancelar",
                "Português" => "Cancelar",
                "中文" => "取消",
                "العربية" => "إلغاء",
                _ => "Annuler",
            },
            "sort_by" => match language {
                "Français" => "Trier par",
                "English" => "Sort by",
                "Deutsch" => "Sortieren nach",
                "Español" => "Ordenar por",
                "Português" => "Ordenar por",
                "中文" => "排序方式",
                "العربية" => "ترتيب حسب",
                _ => "Trier par",
            },
            "sort_name" => match language {
                "Français" => "Nom de style",
                "English" => "Style name",
                "Deutsch" => "Stilname",
                "Español" => "Nombre del estilo",
                "Português" => "Nome do estilo",
                "中文" => "风格名称",
                "العربية" => "اسم النمط",
                _ => "Nom de style",
            },
            "sort_date" => match language {
                "Français" => "Date d’ajout",
                "English" => "Date added",
                "Deutsch" => "Hinzufügedatum",
                "Español" => "Fecha de añadido",
                "Português" => "Data de adição",
                "中文" => "添加日期",
                "العربية" => "تاريخ الإضافة",
                _ => "Date d’ajout",
            },
            "sort_az" => match language {
                "Français" => "A–Z",
                "English" => "A–Z",
                "Deutsch" => "A–Z",
                "Español" => "A–Z",
                "Português" => "A–Z",
                "中文" => "A–Z",
                "العربية" => "A–Z",
                _ => "A–Z",
            },
            "sort_za" => match language {
                "Français" => "Z–A",
                "English" => "Z–A",
                "Deutsch" => "Z–A",
                "Español" => "Z–A",
                "Português" => "Z–A",
                "中文" => "Z–A",
                "العربية" => "Z–A",
                _ => "Z–A",
            },
            "sort_newest" => match language {
                "Français" => "Plus récent",
                "English" => "Newest",
                "Deutsch" => "Neueste",
                "Español" => "Más recientes",
                "Português" => "Mais recentes",
                "中文" => "最新",
                "العربية" => "الأحدث",
                _ => "Plus récent",
            },
            "sort_oldest" => match language {
                "Français" => "Plus ancien",
                "English" => "Oldest",
                "Deutsch" => "Älteste",
                "Español" => "Más antiguos",
                "Português" => "Mais antigos",
                "中文" => "最早",
                "العربية" => "الأقدم",
                _ => "Plus ancien",
            },
            "main_title" => match language {
                "Français" => "♫ Mes styles Suno",
                "English" => "♫ My Suno Styles",
                "Deutsch" => "♫ Meine Suno-Stile",
                "Español" => "♫ Mis estilos Suno",
                "Português" => "♫ Meus estilos Suno",
                "中文" => "♫ 我的 Suno 风格",
                "العربية" => "♫ أنماط Suno الخاصة بي",
                _ => "♫ Mes styles Suno",
            },
            "main_help" => match language {
                "Français" => "Sélectionnez une ligne pour l’éditer ou la supprimer. Cliquez sur ☆ pour ajouter un favori.",
                "English" => "Select a row to edit or delete it. Click ☆ to add a favorite.",
                "Deutsch" => "Wählen Sie eine Zeile zum Bearbeiten oder Löschen. Klicken Sie auf ☆, um sie zu favorisieren.",
                "Español" => "Selecciona una fila para editarla o eliminarla. Haz clic en ☆ para añadirla a favoritos.",
                "Português" => "Selecione uma linha para editar ou excluir. Clique em ☆ para adicionar aos favoritos.",
                "中文" => "选择一行进行编辑或删除。点击 ☆ 可添加到收藏。",
                "العربية" => "حدد صفًا لتعديله أو حذفه. انقر على ☆ لإضافته إلى المفضلة.",
                _ => "Sélectionnez une ligne pour l’éditer ou la supprimer. Cliquez sur ☆ pour ajouter un favori.",
            },
            "styles_banner" => match language {
                "Français" => "STYLES", "English" => "STYLES", "Deutsch" => "STILE", "Español" => "ESTILOS", "Português" => "ESTILOS", "中文" => "风格", "العربية" => "الأنماط", _ => "STYLES",
            },
            "manage_banner" => match language {
                "Français" => "GESTION DES STYLES", "English" => "MANAGE STYLES", "Deutsch" => "STILE VERWALTEN", "Español" => "GESTIONAR ESTILOS", "Português" => "GERIR ESTILOS", "中文" => "管理风格", "العربية" => "إدارة الأنماط", _ => "GESTION DES STYLES",
            },
            "transfer_banner" => match language {
                "Français" => "RECHERCHE / IMPORT / EXPORT", "English" => "SEARCH / IMPORT / EXPORT", "Deutsch" => "SUCHE / IMPORT / EXPORT", "Español" => "BÚSQUEDA / IMPORTAR / EXPORTAR", "Português" => "PESQUISA / IMPORTAR / EXPORTAR", "中文" => "搜索 / 导入 / 导出", "العربية" => "بحث / استيراد / تصدير", _ => "RECHERCHE / IMPORT / EXPORT",
            },
            "library_banner" => match language {
                "Français" => "BIBLIOTHÈQUE",
                "English" => "LIBRARY",
                "Deutsch" => "BIBLIOTHEK",
                "Español" => "BIBLIOTECA",
                "Português" => "BIBLIOTECA",
                "中文" => "资料库",
                "العربية" => "المكتبة",
                _ => "BIBLIOTHÈQUE",
            },
            "search_banner" => match language {
                "Français" => "RECHERCHE",
                "English" => "SEARCH",
                "Deutsch" => "SUCHE",
                "Español" => "BÚSQUEDA",
                "Português" => "PESQUISA",
                "中文" => "搜索",
                "العربية" => "البحث",
                _ => "RECHERCHE",
            },
            "preferences_title" => match language {
                "Français" => "Préférences de l’application",
                "English" => "Application preferences",
                "Deutsch" => "Anwendungseinstellungen",
                "Español" => "Preferencias de la aplicación",
                "Português" => "Preferências da aplicação",
                "中文" => "应用偏好设置",
                "العربية" => "تفضيلات التطبيق",
                _ => "Préférences de l’application",
            },
            "prefs_saved" => match language {
                "Français" => "Les préférences sont sauvegardées sur cet ordinateur.",
                "English" => "Preferences are saved on this computer.",
                "Deutsch" => "Die Einstellungen werden auf diesem Computer gespeichert.",
                "Español" => "Las preferencias se guardan en este equipo.",
                "Português" => "As preferências são guardadas neste computador.",
                "中文" => "偏好设置保存在此电脑上。",
                "العربية" => "يتم حفظ التفضيلات على هذا الكمبيوتر.",
                _ => "Les préférences sont sauvegardées sur cet ordinateur.",
            },
            "search_label" => match language {
                "Français" => "Recherche",
                "English" => "Search",
                "Deutsch" => "Suche",
                "Español" => "Búsqueda",
                "Português" => "Pesquisa",
                "中文" => "搜索",
                "العربية" => "البحث",
                _ => "Recherche",
            },
            "search_hint" => match language {
                "Français" => "Saisir les mots à rechercher…",
                "English" => "Enter words to search…",
                "Deutsch" => "Suchbegriffe eingeben…",
                "Español" => "Introduce palabras para buscar…",
                "Português" => "Introduza palavras para pesquisar…",
                "中文" => "输入要搜索的词…",
                "العربية" => "أدخل كلمات للبحث…",
                _ => "Saisir les mots à rechercher…",
            },
            "light" => match language {
                "Français" => "Mode clair",
                "English" => "Light mode",
                "Deutsch" => "Heller Modus",
                "Español" => "Modo claro",
                "Português" => "Modo claro",
                "中文" => "浅色模式",
                "العربية" => "الوضع الفاتح",
                _ => "Mode clair",
            },
            "dark" => match language {
                "Français" => "Mode sombre",
                "English" => "Dark mode",
                "Deutsch" => "Dunkler Modus",
                "Español" => "Modo oscuro",
                "Português" => "Modo escuro",
                "中文" => "深色模式",
                "العربية" => "الوضع الداكن",
                _ => "Mode sombre",
            },
            "system" => match language {
                "Français" => "Même que l’OS",
                "English" => "Same as OS",
                "Deutsch" => "Wie Betriebssystem",
                "Español" => "Igual que el sistema",
                "Português" => "Igual ao sistema operativo",
                "中文" => "跟随系统",
                "العربية" => "مثل نظام التشغيل",
                _ => "Même que l’OS",
            },
            "tooltip_new" => match language {
                "Français" => "Créer un nouveau style",
                "English" => "Create a new style",
                "Deutsch" => "Neuen Stil erstellen",
                "Español" => "Crear un estilo nuevo",
                "Português" => "Criar um novo estilo",
                "中文" => "创建新风格",
                "العربية" => "إنشاء نمط جديد",
                _ => "Créer un nouveau style",
            },
            "tooltip_edit" => match language {
                "Français" => "Éditer le style sélectionné",
                "English" => "Edit the selected style",
                "Deutsch" => "Ausgewählten Stil bearbeiten",
                "Español" => "Editar el estilo seleccionado",
                "Português" => "Editar o estilo selecionado",
                "中文" => "编辑所选风格",
                "العربية" => "تعديل النمط المحدد",
                _ => "Éditer le style sélectionné",
            },
            "tooltip_delete" => match language {
                "Français" => "Supprimer le style sélectionné",
                "English" => "Delete the selected style",
                "Deutsch" => "Ausgewählten Stil löschen",
                "Español" => "Eliminar el estilo seleccionado",
                "Português" => "Excluir o estilo selecionado",
                "中文" => "删除所选风格",
                "العربية" => "حذف النمط المحدد",
                _ => "Supprimer le style sélectionné",
            },
            "tooltip_import" => match language {
                "Français" => "Importer des styles depuis un fichier JSON",
                "English" => "Import styles from a JSON file",
                "Deutsch" => "Stile aus einer JSON-Datei importieren",
                "Español" => "Importar estilos desde un archivo JSON",
                "Português" => "Importar estilos de um ficheiro JSON",
                "中文" => "从 JSON 文件导入风格",
                "العربية" => "استيراد الأنماط من ملف JSON",
                _ => "Importer des styles depuis un fichier JSON",
            },
            "tooltip_export" => match language {
                "Français" => "Exporter tous les styles dans un fichier JSON",
                "English" => "Export all styles to a JSON file",
                "Deutsch" => "Alle Stile in eine JSON-Datei exportieren",
                "Español" => "Exportar todos los estilos a un archivo JSON",
                "Português" => "Exportar todos os estilos para um ficheiro JSON",
                "中文" => "将所有风格导出为 JSON 文件",
                "العربية" => "تصدير جميع الأنماط إلى ملف JSON",
                _ => "Exporter tous les styles dans un fichier JSON",
            },
            "tooltip_settings" => match language {
                "Français" => "Paramètres de langue et d’affichage",
                "English" => "Language and display settings",
                "Deutsch" => "Sprach- und Anzeigeeinstellungen",
                "Español" => "Configuración de idioma y visualización",
                "Português" => "Definições de idioma e visualização",
                "中文" => "语言和显示设置",
                "العربية" => "إعدادات اللغة والعرض",
                _ => "Paramètres de langue et d’affichage",
            },
            "tooltip_name" => match language {
                "Français" => "Double-cliquez pour éditer ce style",
                "English" => "Double-click to edit this style",
                "Deutsch" => "Doppelklicken, um diesen Stil zu bearbeiten",
                "Español" => "Haz doble clic para editar este estilo",
                "Português" => "Faça duplo clique para editar este estilo",
                "中文" => "双击以编辑此风格",
                "العربية" => "انقر نقرًا مزدوجًا لتعديل هذا النمط",
                _ => "Double-cliquez pour éditer ce style",
            },
            "tooltip_tags" => match language {
                "Français" => "Double-cliquez pour éditer les tags",
                "English" => "Double-click to edit tags",
                "Deutsch" => "Doppelklicken, um Tags zu bearbeiten",
                "Español" => "Haz doble clic para editar las etiquetas",
                "Português" => "Faça duplo clique para editar as tags",
                "中文" => "双击以编辑标签",
                "العربية" => "انقر نقرًا مزدوجًا لتعديل الوسوم",
                _ => "Double-cliquez pour éditer les tags",
            },
            "tooltip_style" => match language {
                "Français" => "Double-cliquez pour éditer le prompt Style",
                "English" => "Double-click to edit the Style prompt",
                "Deutsch" => "Doppelklicken, um den Stil-Prompt zu bearbeiten",
                "Español" => "Haz doble clic para editar el prompt de estilo",
                "Português" => "Faça duplo clique para editar o prompt de estilo",
                "中文" => "双击以编辑风格提示词",
                "العربية" => "انقر نقرًا مزدوجًا لتعديل وصف النمط",
                _ => "Double-cliquez pour éditer le prompt Style",
            },
            "tooltip_exclusion" => match language {
                "Français" => "Double-cliquez pour éditer les exclusions",
                "English" => "Double-click to edit exclusions",
                "Deutsch" => "Doppelklicken, um Ausschlüsse zu bearbeiten",
                "Español" => "Haz doble clic para editar las exclusiones",
                "Português" => "Faça duplo clique para editar as exclusões",
                "中文" => "双击以编辑排除项",
                "العربية" => "انقر نقرًا مزدوجًا لتعديل الاستثناءات",
                _ => "Double-cliquez pour éditer les exclusions",
            },
            "copy_style" => match language {
                "Français" => "📋 Copier le style",
                "English" => "📋 Copy style",
                "Deutsch" => "📋 Stil kopieren",
                "Español" => "📋 Copiar estilo",
                "Português" => "📋 Copiar estilo",
                "中文" => "📋 复制风格",
                "العربية" => "📋 نسخ النمط",
                _ => "📋 Copier le style",
            },
            "copy_exclusion" => match language {
                "Français" => "📋 Copier l’exclusion",
                "English" => "📋 Copy exclusions",
                "Deutsch" => "📋 Ausschlüsse kopieren",
                "Español" => "📋 Copiar exclusiones",
                "Português" => "📋 Copiar exclusões",
                "中文" => "📋 复制排除项",
                "العربية" => "📋 نسخ الاستثناءات",
                _ => "📋 Copier l’exclusion",
            },
            "favorite_add" => match language {
                "Français" => "Ajouter aux favoris",
                "English" => "Add to favorites",
                "Deutsch" => "Zu Favoriten hinzufügen",
                "Español" => "Añadir a favoritos",
                "Português" => "Adicionar aos favoritos",
                "中文" => "添加到收藏",
                "العربية" => "إضافة إلى المفضلة",
                _ => "Ajouter aux favoris",
            },
            "favorite_remove" => match language {
                "Français" => "Retirer des favoris",
                "English" => "Remove from favorites",
                "Deutsch" => "Aus Favoriten entfernen",
                "Español" => "Quitar de favoritos",
                "Português" => "Remover dos favoritos",
                "中文" => "从收藏中移除",
                "العربية" => "إزالة من المفضلة",
                _ => "Retirer des favoris",
            },
            "new_title" => match language {
                "Français" => "Nouveau style",
                "English" => "New style",
                "Deutsch" => "Neuer Stil",
                "Español" => "Nuevo estilo",
                "Português" => "Novo estilo",
                "中文" => "新建风格",
                "العربية" => "نمط جديد",
                _ => "Nouveau style",
            },
            "editor_title" => match language {
                "Français" => "Éditer le style",
                "English" => "Edit style",
                "Deutsch" => "Stil bearbeiten",
                "Español" => "Editar estilo",
                "Português" => "Editar estilo",
                "中文" => "编辑风格",
                "العربية" => "تعديل النمط",
                _ => "Éditer le style",
            },
            "name_label" => match language {
                "Français" => "Nom du style *",
                "English" => "Style name *",
                "Deutsch" => "Stilname *",
                "Español" => "Nombre del estilo *",
                "Português" => "Nome do estilo *",
                "中文" => "风格名称 *",
                "العربية" => "اسم النمط *",
                _ => "Nom du style *",
            },
            "suno_link" => match language {
                "Français" => "Lien Suno",
                "English" => "Suno link",
                "Deutsch" => "Suno-Link",
                "Español" => "Enlace de Suno",
                "Português" => "Link do Suno",
                "中文" => "Suno 链接",
                "العربية" => "رابط Suno",
                _ => "Lien Suno",
            },
            "show_suno_link" => match language {
                "Français" => "Afficher la colonne « Lien Suno » (liens analysés)",
                "English" => "Show the “Suno link” column (analyzed links)",
                "Deutsch" => "Spalte „Suno-Link“ anzeigen (analysierte Links)",
                "Español" => "Mostrar la columna « Enlace de Suno » (enlaces analizados)",
                "Português" => "Mostrar a coluna « Link do Suno » (links analisados)",
                "中文" => "显示“Suno 链接”列（已分析的链接）",
                "العربية" => "إظهار عمود رابط Suno (الروابط التي تم تحليلها)",
                _ => "Afficher la colonne « Lien Suno » (liens analysés)",
            },
            "author" => match language {
                "Français" => "Auteur",
                "English" => "Author",
                "Deutsch" => "Autor",
                "Español" => "Autor",
                "Português" => "Autor",
                "中文" => "作者",
                "العربية" => "المؤلف",
                _ => "Auteur",
            },
            "song_link" => match language {
                "Français" => "Lien vers la chanson Suno (facultatif)",
                "English" => "Suno song link (optional)",
                "Deutsch" => "Suno-Song-Link (optional)",
                "Español" => "Enlace de la canción de Suno (opcional)",
                "Português" => "Link da música Suno (opcional)",
                "中文" => "Suno 歌曲链接（可选）",
                "العربية" => "رابط أغنية Suno (اختياري)",
                _ => "Lien vers la chanson Suno (facultatif)",
            },
            "duplicate_suno_entry" => match language {
                "Français" => "L'entrée existe déjà",
                "English" => "This entry already exists",
                "Deutsch" => "Dieser Eintrag existiert bereits",
                "Español" => "La entrada ya existe",
                "Português" => "Esta entrada já existe",
                "中文" => "该条目已存在",
                "العربية" => "هذا الإدخال موجود بالفعل",
                _ => "L'entrée existe déjà",
            },
            "analyze" => match language {
                "Français" => "Analyser le lien",
                "English" => "Analyze link",
                "Deutsch" => "Link analysieren",
                "Español" => "Analizar enlace",
                "Português" => "Analisar link",
                "中文" => "解析链接",
                "العربية" => "تحليل الرابط",
                _ => "Analyser le lien",
            },
            "tags_label" => match language {
                "Français" => "Tags du champs Lyrics",
                "English" => "Lyrics field tags",
                "Deutsch" => "Tags des Lyrics-Feldes",
                "Español" => "Etiquetas del campo Lyrics",
                "Português" => "Tags do campo Lyrics",
                "中文" => "歌词字段标签",
                "العربية" => "وسوم حقل الكلمات",
                _ => "Tags du champs Lyrics",
            },
            "music_style_label" => match language {
                "Français" => "Style Musical (séparés par des virgules, par exemple : folk, acoustique, mélancolique)",
                "English" => "Musical style (comma-separated, e.g. folk, acoustic, melancholic)",
                "Deutsch" => "Musikstil (durch Kommas getrennt, z. B. folk, akustisch, melancholisch)",
                "Español" => "Estilo musical (separado por comas, por ejemplo: folk, acústico, melancólico)",
                "Português" => "Estilo musical (separado por vírgulas, por exemplo: folk, acústico, melancólico)",
                "中文" => "音乐风格（用逗号分隔，例如：民谣、原声、忧郁）",
                "العربية" => "النمط الموسيقي (مفصول بفواصل، مثل: folk, acoustic, melancholic)",
                _ => "Style Musical (séparés par des virgules, par exemple : folk, acoustique, mélancolique)",
            },
            "tags_hint" => match language {
                "Français" => "séparés par des virgules, par exemple",
                "English" => "comma-separated, for example",
                "Deutsch" => "durch Kommas getrennt, zum Beispiel",
                "Español" => "separadas por comas, por ejemplo",
                "Português" => "separadas por vírgulas, por exemplo",
                "中文" => "用逗号分隔，例如",
                "العربية" => "مفصولة بفواصل، مثل",
                _ => "séparés par des virgules, par exemple",
            },
            "style_label" => match language {
                "Français" => "Style — prompt Suno",
                "English" => "Style — Suno prompt",
                "Deutsch" => "Stil — Suno-Prompt",
                "Español" => "Estilo — prompt de Suno",
                "Português" => "Estilo — prompt do Suno",
                "中文" => "风格 — Suno 提示词",
                "العربية" => "النمط — وصف Suno",
                _ => "Style — prompt Suno",
            },
            "style_hint" => match language {
                "Français" => "Décrivez ici le style musical…",
                "English" => "Describe the musical style here…",
                "Deutsch" => "Beschreiben Sie hier den Musikstil…",
                "Español" => "Describe aquí el estilo musical…",
                "Português" => "Descreva aqui o estilo musical…",
                "中文" => "在此描述音乐风格…",
                "العربية" => "صف النمط الموسيقي هنا…",
                _ => "Décrivez ici le style musical…",
            },
            "exclusion_label" => match language {
                "Français" => "Exclusion — éléments à exclure dans Suno",
                "English" => "Exclusions — elements to exclude in Suno",
                "Deutsch" => "Ausschlüsse — in Suno auszuschließende Elemente",
                "Español" => "Exclusiones — elementos que se deben excluir en Suno",
                "Português" => "Exclusões — elementos a excluir no Suno",
                "中文" => "排除项 — 在 Suno 中排除的元素",
                "العربية" => "الاستثناءات — عناصر يجب استبعادها في Suno",
                _ => "Exclusion — éléments à exclure dans Suno",
            },
            "exclusion_hint" => match language {
                "Français" => "Éléments à éviter…",
                "English" => "Elements to avoid…",
                "Deutsch" => "Zu vermeidende Elemente…",
                "Español" => "Elementos que se deben evitar…",
                "Português" => "Elementos a evitar…",
                "中文" => "需要避免的元素…",
                "العربية" => "عناصر يجب تجنبها…",
                _ => "Éléments à éviter…",
            },
            "delete_title" => match language {
                "Français" => "Confirmer la suppression",
                "English" => "Confirm deletion",
                "Deutsch" => "Löschen bestätigen",
                "Español" => "Confirmar eliminación",
                "Português" => "Confirmar exclusão",
                "中文" => "确认删除",
                "العربية" => "تأكيد الحذف",
                _ => "Confirmer la suppression",
            },
            "delete_prompt" => match language {
                "Français" => "Voulez-vous vraiment supprimer « {name} » ? Cette action est irréversible.",
                "English" => "Are you sure you want to delete “{name}”? This action cannot be undone.",
                "Deutsch" => "Möchten Sie „{name}“ wirklich löschen? Dieser Vorgang kann nicht rückgängig gemacht werden.",
                "Español" => "¿Seguro que quieres eliminar «{name}»? Esta acción es irreversible.",
                "Português" => "Tem a certeza de que pretende excluir «{name}»? Esta ação é irreversível.",
                "中文" => "确定要删除“{name}”吗？此操作无法撤销。",
                "العربية" => "هل تريد حقًا حذف «{name}»؟ لا يمكن التراجع عن هذا الإجراء.",
                _ => "Voulez-vous vraiment supprimer « {name} » ? Cette action est irréversible.",
            },
            "delete_confirm" => match language {
                "Français" => "Supprimer définitivement",
                "English" => "Delete permanently",
                "Deutsch" => "Endgültig löschen",
                "Español" => "Eliminar definitivamente",
                "Português" => "Excluir definitivamente",
                "中文" => "永久删除",
                "العربية" => "حذف نهائيًا",
                _ => "Supprimer définitivement",
            },
            "import_mode_title" => match language {
                "Français" => "Mode d’importation",
                "English" => "Import mode",
                "Deutsch" => "Importmodus",
                "Español" => "Modo de importación",
                "Português" => "Modo de importação",
                "中文" => "导入模式",
                "العربية" => "وضع الاستيراد",
                _ => "Mode d’importation",
            },
            "merge" => match language {
                "Français" => "Fusionner",
                "English" => "Merge",
                "Deutsch" => "Zusammenführen",
                "Español" => "Fusionar",
                "Português" => "Mesclar",
                "中文" => "合并",
                "العربية" => "دمج",
                _ => "Fusionner",
            },
            "merge_description" => match language {
                "Français" => "Ajouter uniquement les nouveaux styles ; les doublons seront ignorés.",
                "English" => "Add only new styles; duplicates will be skipped.",
                "Deutsch" => "Nur neue Stile hinzufügen; Duplikate werden übersprungen.",
                "Español" => "Añadir solo estilos nuevos; se omitirán los duplicados.",
                "Português" => "Adicionar apenas estilos novos; os duplicados serão ignorados.",
                "中文" => "仅添加新风格；重复项将被跳过。",
                "العربية" => "إضافة الأنماط الجديدة فقط؛ سيتم تجاهل المكرر.",
                _ => "Ajouter uniquement les nouveaux styles ; les doublons seront ignorés.",
            },
            "overwrite" => match language {
                "Français" => "Écraser",
                "English" => "Overwrite",
                "Deutsch" => "Ersetzen",
                "Español" => "Sobrescribir",
                "Português" => "Substituir",
                "中文" => "覆盖",
                "العربية" => "استبدال",
                _ => "Écraser",
            },
            "overwrite_description" => match language {
                "Français" => "Remplacer toute la bibliothèque par le contenu du fichier choisi.",
                "English" => "Replace the entire library with the contents of the selected file.",
                "Deutsch" => "Die gesamte Bibliothek durch den Inhalt der ausgewählten Datei ersetzen.",
                "Español" => "Reemplazar toda la biblioteca por el contenido del archivo seleccionado.",
                "Português" => "Substituir toda a biblioteca pelo conteúdo do ficheiro selecionado.",
                "中文" => "用所选文件的内容替换整个资料库。",
                "العربية" => "استبدال المكتبة بالكامل بمحتويات الملف المحدد.",
                _ => "Remplacer toute la bibliothèque par le contenu du fichier choisi.",
            },
            "overwrite_warning" => match language {
                "Français" => "Attention : les styles actuellement enregistrés seront supprimés.",
                "English" => "Warning: all currently saved styles will be deleted.",
                "Deutsch" => "Achtung: Alle aktuell gespeicherten Stile werden gelöscht.",
                "Español" => "Atención: se eliminarán todos los estilos guardados actualmente.",
                "Português" => "Atenção: todos os estilos atualmente guardados serão eliminados.",
                "中文" => "注意：当前保存的所有风格都将被删除。",
                "العربية" => "تحذير: سيتم حذف جميع الأنماط المحفوظة حاليًا.",
                _ => "Attention : les styles actuellement enregistrés seront supprimés.",
            },
            "import_report_title" => match language {
                "Français" => "Rapport d’importation",
                "English" => "Import report",
                "Deutsch" => "Importbericht",
                "Español" => "Informe de importación",
                "Português" => "Relatório de importação",
                "中文" => "导入报告",
                "العربية" => "تقرير الاستيراد",
                _ => "Rapport d’importation",
            },
            "import_report_merge" => match language {
                "Français" => "Fusion terminée.",
                "English" => "Merge completed.",
                "Deutsch" => "Zusammenführen abgeschlossen.",
                "Español" => "Fusión completada.",
                "Português" => "Mesclagem concluída.",
                "中文" => "合并完成。",
                "العربية" => "اكتمل الدمج.",
                _ => "Fusion terminée.",
            },
            "import_report_overwrite" => match language {
                "Français" => "Remplacement terminé.",
                "English" => "Replacement completed.",
                "Deutsch" => "Ersetzen abgeschlossen.",
                "Español" => "Reemplazo completado.",
                "Português" => "Substituição concluída.",
                "中文" => "替换完成。",
                "العربية" => "اكتمل الاستبدال.",
                _ => "Remplacement terminé.",
            },
            "import_report_added" => match language {
                "Français" => "Entrées ajoutées : {count}",
                "English" => "Entries added: {count}",
                "Deutsch" => "Hinzugefügte Einträge: {count}",
                "Español" => "Entradas añadidas: {count}",
                "Português" => "Entradas adicionadas: {count}",
                "中文" => "已添加条目：{count}",
                "العربية" => "الإدخالات المضافة: {count}",
                _ => "Entrées ajoutées : {count}",
            },
            "import_report_skipped" => match language {
                "Français" => "Entrées ignorées (nom vide ou doublon) : {count}",
                "English" => "Entries skipped (empty name or duplicate): {count}",
                "Deutsch" => "Übersprungene Einträge (leerer Name oder Duplikat): {count}",
                "Español" => "Entradas omitidas (nombre vacío o duplicado): {count}",
                "Português" => "Entradas ignoradas (nome vazio ou duplicado): {count}",
                "中文" => "已跳过条目（名称为空或重复）：{count}",
                "العربية" => "الإدخالات التي تم تخطيها (اسم فارغ أو مكرر): {count}",
                _ => "Entrées ignorées (nom vide ou doublon) : {count}",
            },
            "count" => match language {
                "Français" => "{shown} style(s) affiché(s) sur {total} style(s)",
                "English" => "{shown} style(s) shown out of {total}",
                "Deutsch" => "{shown} Stil(e) von {total} angezeigt",
                "Español" => "{shown} de {total} estilos mostrados",
                "Português" => "{shown} de {total} estilos apresentados",
                "中文" => "显示 {shown} / {total} 个风格",
                "العربية" => "عرض {shown} من أصل {total} نمطًا",
                _ => "{shown} style(s) affiché(s) sur {total} style(s)",
            },
            "ready" => match language { "Français" => "Prêt", "English" => "Ready", "Deutsch" => "Bereit", "Español" => "Listo", "Português" => "Pronto", "中文" => "就绪", "العربية" => "جاهز", _ => "Prêt" },
            "styles_section" => match language { "Français" => "Styles", "English" => "Styles", "Deutsch" => "Stile", "Español" => "Estilos", "Português" => "Estilos", "中文" => "风格", "العربية" => "الأنماط", _ => "Styles" },
            "remove_music_filter" => match language { "Français" => "Retirer le filtre de style musical", "English" => "Remove music-style filter", "Deutsch" => "Musikstilfilter entfernen", "Español" => "Quitar el filtro de estilo musical", "Português" => "Remover o filtro de estilo musical", "中文" => "移除音乐风格筛选", "العربية" => "إزالة عامل تصفية النمط الموسيقي", _ => "Retirer le filtre de style musical" },
            "open_suno" => match language { "Français" => "↗ Ouvrir Suno", "English" => "↗ Open Suno", "Deutsch" => "↗ Suno öffnen", "Español" => "↗ Abrir Suno", "Português" => "↗ Abrir Suno", "中文" => "↗ 打开 Suno", "العربية" => "↗ فتح Suno", _ => "↗ Ouvrir Suno" },
            "open_suno_tooltip" => match language { "Français" => "Ouvrir la chanson Suno : {url}", "English" => "Open Suno song: {url}", "Deutsch" => "Suno-Song öffnen: {url}", "Español" => "Abrir canción de Suno: {url}", "Português" => "Abrir música do Suno: {url}", "中文" => "打开 Suno 歌曲：{url}", "العربية" => "فتح أغنية Suno: {url}", _ => "Ouvrir la chanson Suno : {url}" },
            "filter_tag_tooltip" => match language { "Français" => "Filtrer les styles contenant le tag « {tag} »", "English" => "Filter styles containing the tag “{tag}”", "Deutsch" => "Stile mit dem Tag „{tag}“ filtern", "Español" => "Filtrar estilos que contienen la etiqueta «{tag}»", "Português" => "Filtrar estilos que contenham a etiqueta «{tag}»", "中文" => "筛选包含标签“{tag}”的风格", "العربية" => "تصفية الأنماط التي تحتوي على الوسم «{tag}»", _ => "Filtrer les styles contenant le tag « {tag} »" },
            "style_added" => match language { "Français" => "Style ajouté.", "English" => "Style added.", "Deutsch" => "Stil hinzugefügt.", "Español" => "Estilo añadido.", "Português" => "Estilo adicionado.", "中文" => "已添加风格。", "العربية" => "تمت إضافة النمط.", _ => "Style ajouté." },
            "style_updated" => match language { "Français" => "Style modifié.", "English" => "Style updated.", "Deutsch" => "Stil aktualisiert.", "Español" => "Estilo actualizado.", "Português" => "Estilo atualizado.", "中文" => "已更新风格。", "العربية" => "تم تحديث النمط.", _ => "Style modifié." },
            "style_deleted" => match language { "Français" => "Style supprimé.", "English" => "Style deleted.", "Deutsch" => "Stil gelöscht.", "Español" => "Estilo eliminado.", "Português" => "Estilo eliminado.", "中文" => "已删除风格。", "العربية" => "تم حذف النمط.", _ => "Style supprimé." },
            "favorite_added" => match language { "Français" => "Style ajouté aux favoris.", "English" => "Style added to favorites.", "Deutsch" => "Stil zu Favoriten hinzugefügt.", "Español" => "Estilo añadido a favoritos.", "Português" => "Estilo adicionado aos favoritos.", "中文" => "已添加到收藏。", "العربية" => "تمت إضافة النمط إلى المفضلة.", _ => "Style ajouté aux favoris." },
            "favorite_removed" => match language { "Français" => "Style retiré des favoris.", "English" => "Style removed from favorites.", "Deutsch" => "Stil aus Favoriten entfernt.", "Español" => "Estilo eliminado de favoritos.", "Português" => "Estilo removido dos favoritos.", "中文" => "已从收藏中移除。", "العربية" => "تمت إزالة النمط من المفضلة.", _ => "Style retiré des favoris." },
            "edit_select_style" => match language { "Français" => "Sélectionnez d'abord un style à éditer.", "English" => "Select a style to edit first.", "Deutsch" => "Wählen Sie zuerst einen Stil zum Bearbeiten aus.", "Español" => "Primero selecciona un estilo para editar.", "Português" => "Selecione primeiro um estilo para editar.", "中文" => "请先选择要编辑的风格。", "العربية" => "يرجى تحديد نمط لتحريره أولاً.", _ => "Sélectionnez d'abord un style à éditer." },
            "name_required" => match language { "Français" => "Le nom du style est obligatoire.", "English" => "The style name is required.", "Deutsch" => "Der Stilname ist erforderlich.", "Español" => "El nombre del estilo es obligatorio.", "Português" => "O nome do estilo é obrigatório.", "中文" => "风格名称为必填项。", "العربية" => "اسم النمط مطلوب.", _ => "Le nom du style est obligatoire." },
            "duplicate_name" => match language { "Français" => "Un style porte déjà ce nom. Choisissez un nom différent.", "English" => "A style with this name already exists. Choose a different name.", "Deutsch" => "Ein Stil mit diesem Namen existiert bereits. Wählen Sie einen anderen Namen.", "Español" => "Ya existe un estilo con este nombre. Elige otro nombre.", "Português" => "Já existe um estilo com este nome. Escolha outro nome.", "中文" => "已存在同名风格，请选择其他名称。", "العربية" => "يوجد نمط بهذا الاسم بالفعل. اختر اسماً آخر.", _ => "Un style porte déjà ce nom. Choisissez un nom différent." },
            "options_saved" => match language { "Français" => "Options enregistrées.", "English" => "Settings saved.", "Deutsch" => "Einstellungen gespeichert.", "Español" => "Opciones guardadas.", "Português" => "Opções guardadas.", "中文" => "设置已保存。", "العربية" => "تم حفظ الإعدادات.", _ => "Options enregistrées." },
            "suno_analyzed" => match language { "Français" => "Lien Suno analysé : vérifiez les champs avant d’enregistrer.", "English" => "Suno link analyzed: check the fields before saving.", "Deutsch" => "Suno-Link analysiert: Prüfen Sie die Felder vor dem Speichern.", "Español" => "Enlace de Suno analizado: revisa los campos antes de guardar.", "Português" => "Link do Suno analisado: verifique os campos antes de guardar.", "中文" => "Suno 链接已分析：保存前请检查各字段。", "العربية" => "تم تحليل رابط Suno: تحقق من الحقول قبل الحفظ.", _ => "Lien Suno analysé : vérifiez les champs avant d’enregistrer." },
            _ => key,
        }
    }
    fn database_path() -> PathBuf {
        let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
            .or_else(|| std::env::var_os("XDG_DATA_HOME").map(PathBuf::from))
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local").join("share")))
            .unwrap_or_else(|| PathBuf::from("."));
        let dir = base.join("suno-styles");
        let _ = std::fs::create_dir_all(&dir);
        dir.join("suno_styles.sqlite3")
    }

    fn settings_path() -> PathBuf { Self::database_path().with_file_name("settings.json") }

    fn load_settings() -> AppSettings {
        std::fs::read_to_string(Self::settings_path()).ok()
            .and_then(|s| serde_json::from_str::<AppSettings>(&s).ok()).unwrap_or_default()
    }

    fn save_settings(&mut self) {
        match serde_json::to_string_pretty(&self.settings).map_err(|e| e.to_string())
            .and_then(|json| std::fs::write(Self::settings_path(), json).map_err(|e| e.to_string())) {
            Ok(()) => self.status = SunoApp::tr(&self.settings.language, "options_saved").into(),
            Err(e) => self.status = format!("Impossible d’enregistrer les options : {e}"),
        }
    }

    fn delete_suno_links(&mut self) {
        match self.db.execute("UPDATE styles SET suno_url = '' WHERE suno_url <> ''", []) {
            Ok(_) => {
                for style in &mut self.styles { style.suno_url.clear(); }
                self.draft.suno_url.clear();
                self.status = SunoApp::tr(&self.settings.language, "suno_links_deleted").to_string();
            }
            Err(e) => self.status = format!("Impossible de supprimer les liens Suno : {e}"),
        }
    }

    fn new() -> Self {
        let settings = Self::load_settings();
        let path = Self::database_path();
        let db = Connection::open(path).expect("Impossible d'ouvrir la base de données");
        db.execute_batch("CREATE TABLE IF NOT EXISTS styles (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            tags TEXT NOT NULL DEFAULT '',
            music_style TEXT NOT NULL DEFAULT '',
            style TEXT NOT NULL DEFAULT '',
            exclusion TEXT NOT NULL DEFAULT '',
            author TEXT NOT NULL DEFAULT '',
            favorite INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL DEFAULT ''
        );").expect("Impossible de créer la table des styles");
        // Ajoute la colonne aux bases existantes sans perdre les styles enregistrés.
        let has_favorite: bool = db.prepare("PRAGMA table_info(styles)")
            .and_then(|mut stmt| {
                let columns = stmt.query_map([], |row| row.get::<_, String>(1))?;
                let names = columns.collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(names.iter().any(|name| name == "favorite"))
            }).unwrap_or(false);
        if !has_favorite {
            db.execute("ALTER TABLE styles ADD COLUMN favorite INTEGER NOT NULL DEFAULT 0", [])
                .expect("Impossible d'ajouter la colonne des favoris");
        }
        let has_author: bool = db.prepare("PRAGMA table_info(styles)")
            .and_then(|mut stmt| {
                let columns = stmt.query_map([], |row| row.get::<_, String>(1))?;
                let names = columns.collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(names.iter().any(|name| name == "author"))
            }).unwrap_or(false);
        if !has_author {
            db.execute("ALTER TABLE styles ADD COLUMN author TEXT NOT NULL DEFAULT ''", [])
                .expect("Impossible d'ajouter la colonne auteur");
        }
        let has_created_at: bool = db.prepare("PRAGMA table_info(styles)")
            .and_then(|mut stmt| {
                let columns = stmt.query_map([], |row| row.get::<_, String>(1))?;
                let names = columns.collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(names.iter().any(|name| name == "created_at"))
            }).unwrap_or(false);
        if !has_created_at {
            db.execute("ALTER TABLE styles ADD COLUMN created_at TEXT NOT NULL DEFAULT ''", [])
                .expect("Impossible d'ajouter la colonne de date d'ajout");
        }
        let has_suno_url: bool = db.prepare("PRAGMA table_info(styles)")
            .and_then(|mut stmt| {
                let columns = stmt.query_map([], |row| row.get::<_, String>(1))?;
                let names = columns.collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(names.iter().any(|name| name == "suno_url"))
            }).unwrap_or(false);
        if !has_suno_url {
            db.execute("ALTER TABLE styles ADD COLUMN suno_url TEXT NOT NULL DEFAULT ''", [])
                .expect("Impossible d'ajouter la colonne du lien Suno");
        }
        let has_music_style: bool = db.prepare("PRAGMA table_info(styles)")
            .and_then(|mut stmt| {
                let columns = stmt.query_map([], |row| row.get::<_, String>(1))?;
                let names = columns.collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(names.iter().any(|name| name == "music_style"))
            }).unwrap_or(false);
        if !has_music_style {
            db.execute("ALTER TABLE styles ADD COLUMN music_style TEXT NOT NULL DEFAULT ''", [])
                .expect("Impossible d'ajouter la colonne du style musical");
        }
        // Les styles existants reçoivent une date de migration si aucune date n'est connue.
        db.execute("UPDATE styles SET created_at = datetime('now') WHERE created_at = ''", [])
            .expect("Impossible d'initialiser les dates d'ajout");
        let mut app = Self { db, styles: vec![], search: String::new(), search_open: false,
            search_name: true, search_music_style: true, search_tags: true, search_style: true,
            search_exclusion: true, search_author: true, sort_by: "az".into(),
            favorites_only: false, active_music_tag: None, selected_id: None,
            editor_open: false, editor_mode: EditorMode::New, draft: SunoStyle::default(), song_url: String::new(), suno_analysis_checked: false,
            status: SunoApp::tr(&settings.language, "ready").into(), confirm_delete: false, import_dialog_open: false, import_report: None, settings_open: false, settings_about: false, manual_open: false, settings, system_visuals: None };
        app.reload();
        app
    }

    fn reload(&mut self) {
        let result = self.db.prepare("SELECT id, name, tags, music_style, style, exclusion, author, suno_url, favorite, created_at FROM styles")
            .and_then(|mut stmt| {
                let rows = stmt.query_map([], |row| Ok(SunoStyle { id: row.get(0)?, name: row.get(1)?, tags: row.get(2)?, music_style: row.get(3)?, style: row.get(4)?, exclusion: row.get(5)?, author: row.get(6)?, suno_url: row.get(7)?, favorite: row.get::<_, i64>(8)? != 0, created_at: row.get(9)? }))?;
                rows.collect::<rusqlite::Result<Vec<_>>>()
            });
        match result { Ok(rows) => self.styles = rows, Err(e) => self.status = format!("Erreur de lecture : {e}") }
        if !self.styles.iter().any(|s| Some(s.id) == self.selected_id) { self.selected_id = None; }
    }

    fn open_new(&mut self) {
        self.draft = SunoStyle::default();
        self.song_url.clear();
        self.suno_analysis_checked = false;
        self.editor_mode = EditorMode::New;
        self.editor_open = true;
    }

    fn open_edit(&mut self) {
        if let Some(style) = self.styles.iter().find(|s| Some(s.id) == self.selected_id) {
            self.draft = style.clone();
            self.editor_mode = EditorMode::Edit;
            self.editor_open = true;
        } else { self.status = SunoApp::tr(&self.settings.language, "edit_select_style").into(); }
    }

    fn save_draft(&mut self) {
        let name = self.draft.name.trim().to_owned();
        if name.is_empty() {
            self.status = SunoApp::tr(&self.settings.language, "name_required").into();
            return;
        }

        // Lorsqu'un lien Suno a été analysé dans le formulaire de création,
        // ne pas permettre d'enregistrer une entrée déjà présente (par nom ou par URL).
        if self.editor_mode == EditorMode::New
            && self.suno_analysis_checked
            && is_duplicate_suno_entry(&self.styles, &name, &self.song_url)
        {
            self.status = SunoApp::tr(&self.settings.language, "duplicate_suno_entry").to_string();
            return;
        }

        // Évite les doublons créés manuellement, y compris lorsque seule la casse
        // ou les espaces autour du nom diffèrent. En édition, le style courant est exclu.
        let duplicate = self.db.query_row(
            "SELECT EXISTS(SELECT 1 FROM styles WHERE trim(name) = trim(?1) COLLATE NOCASE AND id != ?2)",
            params![name, if self.editor_mode == EditorMode::Edit { self.draft.id } else { -1 }],
            |row| row.get::<_, bool>(0),
        );
        match duplicate {
            Ok(true) => {
                self.status = SunoApp::tr(&self.settings.language, "duplicate_name").into();
                return;
            }
            Err(e) => { self.status = format!("Impossible de vérifier les doublons : {e}"); return; }
            Ok(false) => {}
        }

        let result = match self.editor_mode {
            EditorMode::New => self.db.execute("INSERT INTO styles (name,tags,music_style,style,exclusion,author,suno_url,favorite,created_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,datetime('now'))", params![name, self.draft.tags.trim(), self.draft.music_style.trim(), self.draft.style, self.draft.exclusion, self.draft.author, self.draft.suno_url, self.draft.favorite as i64]),
            EditorMode::Edit => self.db.execute("UPDATE styles SET name=?1,tags=?2,music_style=?3,style=?4,exclusion=?5,author=?6,suno_url=?7,favorite=?8 WHERE id=?9", params![name, self.draft.tags.trim(), self.draft.music_style.trim(), self.draft.style, self.draft.exclusion, self.draft.author, self.draft.suno_url, self.draft.favorite as i64, self.draft.id]),
        };
        match result {
            Ok(_) => { self.status = (if self.editor_mode == EditorMode::New { SunoApp::tr(&self.settings.language, "style_added") } else { SunoApp::tr(&self.settings.language, "style_updated") }).into(); self.editor_open = false; self.reload(); if self.editor_mode == EditorMode::New { self.selected_id = self.styles.iter().find(|s| s.name == name).map(|s| s.id); } }
            Err(e) => self.status = format!("Erreur d'enregistrement : {e}"),
        }
    }

    fn delete_selected(&mut self) {
        if let Some(id) = self.selected_id {
            match self.db.execute("DELETE FROM styles WHERE id=?1", [id]) {
                Ok(_) => { self.selected_id = None; self.confirm_delete = false; self.status = SunoApp::tr(&self.settings.language, "style_deleted").into(); self.reload(); }
                Err(e) => self.status = format!("Erreur de suppression : {e}"),
            }
        }
    }

    fn copy_text(&mut self, text: &str, label: &str) {
        match Clipboard::new().and_then(|mut c| c.set_text(text.to_owned())) {
            Ok(()) => self.status = format!("{label} copié dans le presse-papiers."),
            Err(e) => self.status = format!("Impossible d'accéder au presse-papiers : {e}"),
        }
    }

    fn set_favorite(&mut self, id: i64, favorite: bool) {
        match self.db.execute("UPDATE styles SET favorite=?1 WHERE id=?2", params![favorite as i64, id]) {
            Ok(_) => {
                self.status = (if favorite { SunoApp::tr(&self.settings.language, "favorite_added") } else { SunoApp::tr(&self.settings.language, "favorite_removed") }).into();
                self.reload();
            }
            Err(e) => self.status = format!("Impossible de modifier le favori : {e}"),
        }
    }

    fn export_styles(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Styles Suno (JSON)", &["json"])
            .set_file_name("suno-styles.json")
            .save_file() else { return; };

        let portable: Vec<PortableStyle> = self.styles.iter().map(|s| PortableStyle {
            name: s.name.clone(), tags: s.tags.clone(), music_style: s.music_style.clone(), style: s.style.clone(), exclusion: s.exclusion.clone(), author: s.author.clone(), suno_url: s.suno_url.clone(), favorite: s.favorite,
        }).collect();
        match serde_json::to_string_pretty(&portable)
            .map_err(|e| e.to_string())
            .and_then(|json| std::fs::write(&path, json).map_err(|e| e.to_string())) {
            Ok(()) => self.status = format!("{} style(s) exporté(s) vers {}.", portable.len(), path.display()),
            Err(e) => self.status = format!("Erreur lors de l'export : {e}"),
        }
    }

    fn import_styles(&mut self, overwrite: bool) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Styles Suno (JSON)", &["json"])
            .pick_file() else { return; };

        let contents = match std::fs::read_to_string(&path) {
            Ok(contents) => contents,
            Err(e) => { self.status = format!("Lecture du fichier impossible ({}): {e}", path.display()); return; }
        };
        let imported: Vec<PortableStyle> = match serde_json::from_str(&contents) {
            Ok(styles) => styles,
            Err(e) => {
                self.status = format!(
                    "JSON invalide dans {} (ligne {}, colonne {}) : {}. La bibliothèque n'a pas été modifiée.",
                    path.display(), e.line(), e.column(), e
                );
                return;
            }
        };

        // Import transactionnel : une erreur SQL annule l'ensemble de l'opération.
        // La détection des doublons tient compte de la bibliothèque et du fichier importé.
        let result: rusqlite::Result<(usize, usize)> = (|| {
            let tx = self.db.transaction()?;
            if overwrite { tx.execute("DELETE FROM styles", [])?; }

            let mut names = HashSet::<String>::new();
            if !overwrite {
                let mut stmt = tx.prepare("SELECT name FROM styles")?;
                let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
                for row in rows {
                    if let Some(key) = normalized_name_key(&row?) { names.insert(key); }
                }
            }

            let mut added = 0usize;
            let mut skipped = 0usize;
            for style in imported {
                let Some(name_key) = normalized_name_key(&style.name) else {
                    skipped += 1;
                    continue;
                };
                if !names.insert(name_key) {
                    skipped += 1;
                    continue;
                }
                let display_name = style.name.trim();
                // Conserve tous les champs métier : favoris, auteur, URL Suno, style
                // musical et tags de la zone Lyrics, y compris lorsqu'ils sont vides.
                tx.execute(
                    "INSERT INTO styles (name, tags, music_style, style, exclusion, author, suno_url, favorite, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, datetime('now'))",
                    params![display_name, style.tags, style.music_style, style.style, style.exclusion, style.author, style.suno_url, style.favorite as i64],
                )?;
                added += 1;
            }
            tx.commit()?;
            Ok((added, skipped))
        })();

        match result {
            Ok((added, skipped)) => {
                self.reload();
                self.status = if overwrite {
                    format!("Import terminé (remplacement) : {added} style(s) importé(s), {skipped} ignoré(s) (nom vide ou doublon).")
                } else {
                    format!("Fusion terminée : {added} nouveau(x) style(s) ajouté(s), {skipped} ignoré(s) (nom vide ou doublon).")
                };
                self.import_report = Some((added, skipped, overwrite));
            }
            Err(e) => {
                self.status = format!("Échec de l'import : {e}. La transaction a été annulée et les données précédentes sont conservées.");
                self.reload();
            }
        }
    }
}

fn library_filter_chip(ui: &mut egui::Ui, selected: bool, label: &str, icon: &str) -> egui::Response {
    let width = (label.chars().count() as f32 * 7.1 + 46.0).clamp(118.0, 210.0);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 30.0), egui::Sense::click());
    let visuals = ui.style().interact(&response);
    let fill = if selected { ui.visuals().selection.bg_fill } else if response.hovered() { visuals.bg_fill } else { ui.visuals().panel_fill };
    let stroke = if selected { ui.visuals().selection.stroke } else { ui.visuals().widgets.noninteractive.bg_stroke };
    ui.painter().rect_filled(rect, 7.0, fill);
    ui.painter().rect_stroke(rect, 7.0, stroke, egui::StrokeKind::Outside);
    let icon_color = if selected { ui.visuals().selection.stroke.color } else { ui.visuals().text_color() };
    draw_icon(ui.painter(), egui::pos2(rect.left() + 17.0, rect.center().y), icon, icon_color);
    ui.painter().text(
        egui::pos2(rect.left() + 32.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(12.5),
        if selected { ui.visuals().strong_text_color() } else { ui.visuals().text_color() },
    );
    response
}

impl eframe::App for SunoApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Utilisé pour ne pas interpréter le clic d'ouverture comme un clic extérieur.
        let import_was_open_at_frame_start = self.import_dialog_open;
        let settings_was_open_at_frame_start = self.settings_open;
        let manual_was_open_at_frame_start = self.manual_open;
        let editor_was_open_at_frame_start = self.editor_open;
        if self.system_visuals.is_none() {
            self.system_visuals = Some(ctx.style().visuals.clone());
        }
        match self.settings.theme.as_str() {
            "Mode sombre" => ctx.set_visuals(egui::Visuals::dark()),
            "Mode clair" => ctx.set_visuals(egui::Visuals::light()),
            _ => if let Some(visuals) = &self.system_visuals { ctx.set_visuals(visuals.clone()); },
        }

        // Raccourcis clavier : désactivés pendant l'édition ou lorsqu'un dialogue est ouvert,
        // et lorsque le focus est dans un champ texte pour ne pas détourner la saisie.
        let modal_open = self.editor_open || self.settings_open || self.manual_open || self.import_dialog_open
            || self.import_report.is_some() || self.confirm_delete;
        if !modal_open && !ctx.wants_keyboard_input() {
            let (new_shortcut, edit_shortcut, search_shortcut, delete_shortcut,
                 copy_style_shortcut, copy_exclusion_shortcut, escape_shortcut) = ctx.input(|i| (
                i.modifiers.command && i.key_pressed(egui::Key::N),
                i.modifiers.command && i.key_pressed(egui::Key::E),
                i.modifiers.command && i.key_pressed(egui::Key::F),
                i.key_pressed(egui::Key::Delete),
                i.modifiers.command && !i.modifiers.shift && i.key_pressed(egui::Key::C),
                i.modifiers.command && i.modifiers.shift && i.key_pressed(egui::Key::C),
                i.key_pressed(egui::Key::Escape),
            ));
            if new_shortcut {
                self.open_new();
            } else if edit_shortcut {
                self.open_edit();
            } else if search_shortcut {
                self.search_open = true;
                self.status = "Recherche ouverte (Ctrl+F).".into();
            } else if delete_shortcut && self.selected_id.is_some() {
                self.confirm_delete = true;
            } else if copy_style_shortcut {
                if let Some(style) = self.styles.iter().find(|s| Some(s.id) == self.selected_id).cloned() {
                    self.copy_text(&style.style, "Style");
                }
            } else if copy_exclusion_shortcut {
                if let Some(style) = self.styles.iter().find(|s| Some(s.id) == self.selected_id).cloned() {
                    self.copy_text(&style.exclusion, "Exclusion");
                }
            } else if escape_shortcut && self.search_open {
                self.search_open = false;
                self.search.clear();
            }
        }

        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            egui::Frame::new()
                .fill(ui.visuals().panel_fill)
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.heading(egui::RichText::new("♫ Suno Styles").strong());
                            ui.label(egui::RichText::new(SunoApp::tr(&self.settings.language, "library_banner")).small().weak());
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if icon_button(ui, "settings", SunoApp::tr(&self.settings.language, "options"), 76.0)
                                .on_hover_text(SunoApp::tr(&self.settings.language, "tooltip_settings")).clicked() { self.settings_about = false; self.settings_open = true; }
                            if icon_button(ui, "manual", SunoApp::tr(&self.settings.language, "manual"), 76.0)
                                .on_hover_text(SunoApp::tr(&self.settings.language, "manual_tooltip")).clicked() { self.manual_open = true; }
                        });
                    });
                    ui.add_space(8.0);
                    ui.horizontal_wrapped(|ui| {
                        let has_selection = self.selected_id.is_some();
                        egui::Frame::group(ui.style())
                            .inner_margin(egui::Margin::symmetric(6, 6))
                            .show(ui, |ui| {
                                ui.vertical(|ui| {
                                    ui.label(egui::RichText::new(SunoApp::tr(&self.settings.language, "manage_banner")).small().weak().strong());
                                    ui.horizontal(|ui| {
                                        if icon_button(ui, "new", SunoApp::tr(&self.settings.language, "new"), 78.0)
                                            .on_hover_text(format!("{} (Ctrl+N)", SunoApp::tr(&self.settings.language, "tooltip_new"))).clicked() { self.open_new(); }
                                        if icon_button_enabled(ui, "edit", SunoApp::tr(&self.settings.language, "edit"), 78.0, has_selection)
                                            .on_hover_text(format!("{} (Ctrl+E)", SunoApp::tr(&self.settings.language, "tooltip_edit"))).clicked() { self.open_edit(); }
                                        if icon_button_enabled(ui, "delete", SunoApp::tr(&self.settings.language, "delete"), 78.0, has_selection)
                                            .on_hover_text(format!("{} (Suppr)", SunoApp::tr(&self.settings.language, "tooltip_delete"))).clicked() { self.confirm_delete = true; }
                                    });
                                });
                            });
                        egui::Frame::group(ui.style())
                            .inner_margin(egui::Margin::symmetric(6, 6))
                            .show(ui, |ui| {
                                ui.vertical(|ui| {
                                    ui.label(egui::RichText::new(SunoApp::tr(&self.settings.language, "transfer_banner")).small().weak().strong());
                                    ui.horizontal(|ui| {
                                        if icon_button(ui, "search", SunoApp::tr(&self.settings.language, "search"), 78.0)
                                            .on_hover_text(format!("{} (Ctrl+F)", SunoApp::tr(&self.settings.language, "search_label"))).clicked() {
                                            self.search_open = !self.search_open;
                                            if !self.search_open { self.search.clear(); }
                                        }
                                        if icon_button(ui, "import", SunoApp::tr(&self.settings.language, "import"), 78.0)
                                            .on_hover_text(SunoApp::tr(&self.settings.language, "tooltip_import")).clicked() { self.import_dialog_open = true; }
                                        if icon_button(ui, "export", SunoApp::tr(&self.settings.language, "export"), 78.0)
                                            .on_hover_text(SunoApp::tr(&self.settings.language, "tooltip_export")).clicked() { self.export_styles(); }
                                    });
                                });
                            });
                    });
                });
            if self.search_open {
                egui::Frame::new()
                    .fill(ui.visuals().faint_bg_color)
                    .stroke(egui::Stroke::new(1.0_f32, ui.visuals().widgets.noninteractive.bg_stroke.color))
                    .corner_radius(10.0)
                    .inner_margin(egui::Margin::symmetric(12, 10))
                    .show(ui, |ui| {
                        ui.vertical(|ui| {
                            ui.horizontal(|ui| {
                                // Icône dessinée par l'application : évite les glyphes Unicode
                                // manquants qui s'affichent sous forme de carré selon la police.
                                let (search_icon_rect, _) = ui.allocate_exact_size(egui::vec2(22.0, 22.0), egui::Sense::hover());
                                draw_icon(ui.painter(), search_icon_rect.center(), "search", ui.visuals().text_color());
                                ui.label(egui::RichText::new(SunoApp::tr(&self.settings.language, "search_label")).strong().size(14.0));
                                ui.add_space(8.0);
                                let input_width = (ui.available_width() - 90.0).max(120.0);
                                let response = ui.add_sized(
                                    [input_width, 30.0],
                                    egui::TextEdit::singleline(&mut self.search)
                                        .hint_text(SunoApp::tr(&self.settings.language, "search_hint")),
                                );
                                if response.changed() { ctx.request_repaint(); }
                                if !self.search.is_empty()
                                    && ui.button(format!("✕ {}", SunoApp::tr(&self.settings.language, "clear"))).clicked() {
                                    self.search.clear();
                                    ctx.request_repaint();
                                }
                            });
                            ui.add_space(6.0);
                            ui.horizontal_wrapped(|ui| {
                                ui.label(egui::RichText::new(SunoApp::tr(&self.settings.language, "search_in")).strong().small());
                                ui.checkbox(&mut self.search_name, SunoApp::tr(&self.settings.language, "name"));
                                ui.checkbox(&mut self.search_music_style, SunoApp::tr(&self.settings.language, "music_style"));
                                ui.checkbox(&mut self.search_tags, SunoApp::tr(&self.settings.language, "lyrics_tags"));
                                ui.checkbox(&mut self.search_style, SunoApp::tr(&self.settings.language, "style"));
                                ui.checkbox(&mut self.search_exclusion, SunoApp::tr(&self.settings.language, "exclusion"));
                                ui.checkbox(&mut self.search_author, SunoApp::tr(&self.settings.language, "author"));
                            });
                        });
                    });
            }
        });

        let query = self.search.to_lowercase();
        let active_music_tag = self.active_music_tag.clone();
        let visible: Vec<SunoStyle> = self.styles.iter().filter(|s| (!self.favorites_only || s.favorite)
            && active_music_tag.as_ref().map_or(true, |tag| music_style_has_tag(&s.music_style, tag))
            && (query.is_empty() ||
                (self.search_name && s.name.to_lowercase().contains(&query)) ||
                (self.search_music_style && s.music_style.to_lowercase().contains(&query)) ||
                (self.search_tags && s.tags.to_lowercase().contains(&query)) ||
                (self.search_style && s.style.to_lowercase().contains(&query)) ||
                (self.search_exclusion && s.exclusion.to_lowercase().contains(&query)) ||
                (self.search_author && s.author.to_lowercase().contains(&query)))).cloned().collect();
        let mut visible = visible;
        match self.sort_by.as_str() {
            "za" => visible.sort_by(|a, b| b.name.to_lowercase().cmp(&a.name.to_lowercase())),
            "newest" => visible.sort_by(|a, b| b.created_at.cmp(&a.created_at).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))),
            "oldest" => visible.sort_by(|a, b| a.created_at.cmp(&b.created_at).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))),
            _ => visible.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase())),
        }
        let displayed_count = visible.len();
        let total_count = self.styles.len();

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(8.0);
            egui::Frame::new()
                .fill(ui.visuals().extreme_bg_color)
                .inner_margin(egui::Margin::symmetric(12, 9))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.heading(SunoApp::tr(&self.settings.language, "main_title"));
                            ui.label(egui::RichText::new(SunoApp::tr(&self.settings.language, "main_help")).small().weak());
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(egui::RichText::new(format!("{} / {}", displayed_count, total_count)).strong().size(14.0));
                            ui.label(egui::RichText::new(SunoApp::tr(&self.settings.language, "styles_section")).small().weak());
                        });
                    });
                    ui.add_space(6.0);
                    ui.horizontal_wrapped(|ui| {
                        let all_response = library_filter_chip(ui, !self.favorites_only && self.active_music_tag.is_none(), SunoApp::tr(&self.settings.language, "all"), "library");
                        if all_response.clicked() { self.favorites_only = false; self.active_music_tag = None; }
                        let favorites_response = library_filter_chip(ui, self.favorites_only, SunoApp::tr(&self.settings.language, "favorites"), "favorite");
                        if favorites_response.clicked() { self.favorites_only = true; }
                        if let Some(tag) = self.active_music_tag.clone() {
                            egui::Frame::new()
                                .fill(ui.visuals().selection.bg_fill.gamma_multiply(0.55))
                                .stroke(egui::Stroke::new(1.0_f32, ui.visuals().selection.stroke.color))
                                .corner_radius(8.0)
                                .inner_margin(egui::Margin::symmetric(8, 4))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(egui::RichText::new(format!("♫ {}", tag)).strong());
                                        if ui.small_button("×").on_hover_text(SunoApp::tr(&self.settings.language, "remove_music_filter")).clicked() {
                                            self.active_music_tag = None;
                                            self.favorites_only = false;
                                        }
                                    });
                                });
                        }
                    });
                });
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                ui.label(SunoApp::tr(&self.settings.language, "sort_by"));
                egui::ComboBox::from_id_salt("styles_sort_by")
                    .selected_text(SunoApp::tr(&self.settings.language, match self.sort_by.as_str() { "za" => "sort_za", "newest" => "sort_newest", "oldest" => "sort_oldest", _ => "sort_az" }))
                    .width(140.0)
                    .show_ui(ui, |ui| {
                        if ui.selectable_label(self.sort_by == "az", SunoApp::tr(&self.settings.language, "sort_az")).clicked() { self.sort_by = "az".into(); }
                        if ui.selectable_label(self.sort_by == "za", SunoApp::tr(&self.settings.language, "sort_za")).clicked() { self.sort_by = "za".into(); }
                        if ui.selectable_label(self.sort_by == "newest", SunoApp::tr(&self.settings.language, "sort_newest")).clicked() { self.sort_by = "newest".into(); }
                        if ui.selectable_label(self.sort_by == "oldest", SunoApp::tr(&self.settings.language, "sort_oldest")).clicked() { self.sort_by = "oldest".into(); }
                    });
            });
            ui.add_space(6.0);
            ui.separator();
            ui.add_space(5.0);
            let mut edit_on_double_click: Option<i64> = None;
            let mut favorite_toggle: Option<(i64, bool)> = None;
            // Tableau V33 : dimensions calculées à partir de la largeur réellement disponible.
            // Un Grid à 7 colonnes comporte 6 espaces horizontaux : ils sont tous déduits
            // avant la répartition pour éviter que les colonnes débordent de la fenêtre.
            // La même série de dimensions est utilisée pour l'en-tête et toutes les lignes.
            let spacing_x = 5.0_f32;
            let row_height = 66.0_f32;
            let available = ui.available_width().max(0.0);
            let table_width = (available - 8.0).max(0.0);
            let usable = (table_width - spacing_x * 6.0).max(0.0);
            let favorite_w = 27.0_f32.min(usable);
            let rest = (usable - favorite_w).max(0.0);
            let name_w = rest * 0.22;
            let music_style_w = rest * 0.14;
            let tags_w = rest * 0.15;
            let style_w = rest * 0.20;
            let exclusion_w = rest * 0.19;
            let author_w = rest * 0.10;
            let columns = 7;

            egui::Frame::new()
                .fill(ui.visuals().faint_bg_color)
                .inner_margin(egui::Margin::symmetric(0, 4))
                .show(ui, |ui| {
                    egui::Grid::new("styles_grid_header_v31")
                        .num_columns(columns)
                        .min_col_width(0.0)
                        .spacing(egui::vec2(spacing_x, 4.0))
                        .show(ui, |ui| {
                            ui.add_sized([favorite_w, 22.0], egui::Label::new(egui::RichText::new("★").strong().color(egui::Color32::from_rgb(205, 151, 45))));
                            ui.add_sized([name_w, 28.0], egui::Label::new(egui::RichText::new(SunoApp::tr(&self.settings.language, "name")).strong()).wrap());
                            ui.add_sized([music_style_w, 28.0], egui::Label::new(egui::RichText::new(SunoApp::tr(&self.settings.language, "music_style")).strong()).wrap());
                            ui.add_sized([tags_w, 28.0], egui::Label::new(egui::RichText::new(SunoApp::tr(&self.settings.language, "lyrics_tags")).strong()).wrap());
                            ui.add_sized([style_w, 22.0], egui::Label::new(egui::RichText::new(SunoApp::tr(&self.settings.language, "style")).strong()));
                            ui.add_sized([exclusion_w, 22.0], egui::Label::new(egui::RichText::new(SunoApp::tr(&self.settings.language, "exclusion")).strong()));
                            ui.add_sized([author_w, 22.0], egui::Label::new(egui::RichText::new(SunoApp::tr(&self.settings.language, "author")).strong()));
                            ui.end_row();
                        });
                });

            egui::ScrollArea::vertical()
                .id_salt("styles_table_rows_v31")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    egui::Grid::new("styles_grid_rows_v31")
                        .num_columns(columns)
                        .striped(false)
                        .min_col_width(0.0)
                        .spacing(egui::vec2(spacing_x, 5.0))
                        .show(ui, |ui| {
                            for (row_index, s) in visible.iter().enumerate() {
                                // Fond de ligne continu : alternance discrète et surbrillance de sélection.
                                // Le fond est peint avant les contrôles pour conserver les clics des cellules.
                                let row_rect = egui::Rect::from_min_size(
                                    ui.cursor().min,
                                    egui::vec2(table_width, row_height),
                                );
                                // Zone d'interaction enregistrée avant les cellules : les clics
                                // sur les espaces libres de la ligne sélectionnent aussi l'entrée,
                                // sans masquer les boutons, liens ou cellules interactives.
                                let row_response = ui.interact(
                                    row_rect,
                                    ui.make_persistent_id(("style_row_select", s.id)),
                                    egui::Sense::click(),
                                );
                                if row_response.clicked() {
                                    self.selected_id = Some(s.id);
                                }
                                if row_response.double_clicked() {
                                    self.selected_id = Some(s.id);
                                    edit_on_double_click = Some(s.id);
                                }
                                let is_selected = self.selected_id == Some(s.id);
                                let row_hovered = row_rect.contains(ui.input(|input| input.pointer.hover_pos().unwrap_or(egui::Pos2::new(-1.0, -1.0))));
                                let row_fill = if is_selected {
                                    ui.visuals().selection.bg_fill
                                } else if row_hovered {
                                    ui.visuals().widgets.hovered.bg_fill
                                } else if row_index % 2 == 1 {
                                    ui.visuals().faint_bg_color
                                } else {
                                    egui::Color32::TRANSPARENT
                                };
                                if row_fill != egui::Color32::TRANSPARENT {
                                    ui.painter().rect_filled(row_rect, 5.0, row_fill);
                                }
                                // Favori transparent, sans cadre ni fond permanent, aligné sur le nom.
                                ui.allocate_ui_with_layout(
                                    egui::vec2(favorite_w, row_height),
                                    egui::Layout::top_down(egui::Align::Center),
                                    |ui| {
                                        let star = if s.favorite { "★" } else { "☆" };
                                        let star_color = if s.favorite {
                                            egui::Color32::from_rgb(226, 171, 47)
                                        } else {
                                            ui.visuals().weak_text_color()
                                        };
                                        let response = ui.add_sized(
                                            [favorite_w, 24.0],
                                            egui::Button::new(egui::RichText::new(star).size(21.0).color(star_color)).frame(false),
                                        ).on_hover_text(if s.favorite {
                                            SunoApp::tr(&self.settings.language, "favorite_remove")
                                        } else {
                                            SunoApp::tr(&self.settings.language, "favorite_add")
                                        });
                                        if response.clicked() { favorite_toggle = Some((s.id, !s.favorite)); }
                                    },
                                );

                                ui.allocate_ui_with_layout(
                                    egui::vec2(name_w, row_height),
                                    egui::Layout::top_down(egui::Align::Min),
                                    |ui| {
                                        ui.set_width(name_w);
                                        let name_response = ui.add(
                                            egui::Label::new(egui::RichText::new(&s.name).size(12.5).strong())
                                                .wrap()
                                                .sense(egui::Sense::click()),
                                        ).on_hover_text(SunoApp::tr(&self.settings.language, "tooltip_name"));
                                        if name_response.clicked() { self.selected_id = Some(s.id); }
                                        if name_response.double_clicked() { edit_on_double_click = Some(s.id); }
                                        if !s.suno_url.trim().is_empty() {
                                            ui.add_space(2.0);
                                            egui::Frame::new()
                                                .fill(if is_selected { ui.visuals().selection.bg_fill } else { ui.visuals().faint_bg_color })
                                                .stroke(egui::Stroke::new(0.5_f32, ui.visuals().widgets.noninteractive.bg_stroke.color))
                                                .corner_radius(4.0)
                                                .inner_margin(egui::Margin::symmetric(4, 1))
                                                .show(ui, |ui| {
                                                    ui.hyperlink_to(
                                                        egui::RichText::new(SunoApp::tr(&self.settings.language, "open_suno")).size(10.0),
                                                        &s.suno_url,
                                                    ).on_hover_text(SunoApp::tr(&self.settings.language, "open_suno_tooltip").replace("{url}", &s.suno_url));
                                                });
                                        }
                                    },
                                );

                                ui.allocate_ui_with_layout(
                                    egui::vec2(music_style_w, row_height),
                                    egui::Layout::top_down(egui::Align::Min),
                                    |ui| {
                                        egui::ScrollArea::vertical()
                                            .id_salt(("music_style_cell_v32", s.id))
                                            .max_height((row_height - 6.0).max(24.0))
                                            .auto_shrink([false, false])
                                            .show(ui, |ui| {
                                                ui.set_width(music_style_w - 4.0);
                                                if s.music_style.trim().is_empty() {
                                                    ui.label(egui::RichText::new("—").size(11.0).weak());
                                                } else {
                                                    ui.horizontal_wrapped(|ui| {
                                                        for tag in s.music_style.split(',').map(str::trim).filter(|tag| !tag.is_empty()) {
                                                            let tag_fill = if is_selected { ui.visuals().selection.bg_fill.gamma_multiply(0.72) } else { ui.visuals().faint_bg_color };
                                                            let response = egui::Frame::new()
                                                                .fill(if self.active_music_tag.as_ref().map_or(false, |active| active.eq_ignore_ascii_case(tag)) {
                                                                    ui.visuals().selection.bg_fill
                                                                } else { tag_fill })
                                                                .stroke(egui::Stroke::new(0.5_f32, ui.visuals().widgets.noninteractive.bg_stroke.color))
                                                                .corner_radius(4.0)
                                                                .inner_margin(egui::Margin::symmetric(4, 2))
                                                                .show(ui, |ui| ui.add(egui::Label::new(egui::RichText::new(tag).size(10.5).underline()).wrap().sense(egui::Sense::click())))
                                                                .inner
                                                                .on_hover_text(SunoApp::tr(&self.settings.language, "filter_tag_tooltip").replace("{tag}", tag));
                                                            if response.clicked() {
                                                                self.active_music_tag = Some(tag.to_owned());
                                                                self.favorites_only = false;
                                                                self.selected_id = Some(s.id);
                                                            }
                                                            if response.double_clicked() { edit_on_double_click = Some(s.id); }
                                                        }
                                                    });
                                                }
                                            });
                                    },
                                );

                                ui.allocate_ui_with_layout(
                                    egui::vec2(tags_w, row_height),
                                    egui::Layout::top_down(egui::Align::Min),
                                    |ui| {
                                        egui::ScrollArea::vertical()
                                            .id_salt(("tags_cell_v24", s.id))
                                            .max_height((row_height - 6.0).max(24.0))
                                            .auto_shrink([false, false])
                                            .show(ui, |ui| {
                                                ui.set_width(tags_w - 4.0);
                                                // Les tags simples séparés par des virgules deviennent des pastilles.
                                                // Les tags de paroles multilignes ou entre crochets restent en texte brut.
                                                let compact_tags = !s.tags.contains('\n')
                                                    && !s.tags.contains('[')
                                                    && s.tags.split(',').filter(|tag| !tag.trim().is_empty()).count() <= 10;
                                                if compact_tags && s.tags.contains(',') {
                                                    ui.horizontal_wrapped(|ui| {
                                                        for tag in s.tags.split(',').map(str::trim).filter(|tag| !tag.is_empty()) {
                                                            let tag_fill = if is_selected {
                                                                ui.visuals().selection.bg_fill.gamma_multiply(0.72)
                                                            } else {
                                                                ui.visuals().faint_bg_color
                                                            };
                                                            let response = egui::Frame::new()
                                                                .fill(tag_fill)
                                                                .stroke(egui::Stroke::new(0.5_f32, ui.visuals().widgets.noninteractive.bg_stroke.color))
                                                                .corner_radius(4.0)
                                                                .inner_margin(egui::Margin::symmetric(4, 2))
                                                                .show(ui, |ui| {
                                                                    ui.add(egui::Label::new(egui::RichText::new(tag).size(10.5)).sense(egui::Sense::click()))
                                                                }).inner;
                                                            if response.clicked() { self.selected_id = Some(s.id); }
                                                            if response.double_clicked() { edit_on_double_click = Some(s.id); }
                                                        }
                                                    });
                                                } else {
                                                    let tag_color = if is_selected { ui.visuals().text_color() } else { ui.visuals().weak_text_color() };
                                                    let response = ui.add(egui::Label::new(egui::RichText::new(&s.tags).size(11.5).color(tag_color)).wrap().sense(egui::Sense::click()));
                                                    if response.clicked() { self.selected_id = Some(s.id); }
                                                    if response.double_clicked() { edit_on_double_click = Some(s.id); }
                                                }
                                            });
                                    },
                                );

                                ui.allocate_ui_with_layout(
                                    egui::vec2(style_w, row_height),
                                    egui::Layout::top_down(egui::Align::Min),
                                    |ui| {
                                        egui::ScrollArea::vertical()
                                            .id_salt(("style_cell_v24", s.id))
                                            .max_height(40.0)
                                            .auto_shrink([false, false])
                                            .show(ui, |ui| {
                                                ui.set_width(style_w - 4.0);
                                                let response = ui.add(egui::Label::new(egui::RichText::new(&s.style).size(11.5)).wrap().sense(egui::Sense::click()));
                                                if response.double_clicked() { edit_on_double_click = Some(s.id); }
                                                if response.clicked() { self.selected_id = Some(s.id); }
                                            });
                                        ui.add_space(1.0);
                                        ui.horizontal(|ui| {
                                            ui.spacing_mut().item_spacing.x = 2.0;
                                            let count = s.style.chars().count();
                                            let color = if count > 1000 { egui::Color32::RED } else { ui.visuals().text_color() };
                                            let full_copy_label = SunoApp::tr(&self.settings.language, "copy_style");
                                            let copy_label = if style_w < 190.0 { "📋 Copier" } else { full_copy_label };
                                            let copy_button = egui::Button::new(copy_label).small();
                                            let copy_button = if is_selected {
                                                copy_button.fill(row_fill).stroke(egui::Stroke::NONE)
                                            } else {
                                                copy_button
                                            };
                                            if ui.add_sized([ (style_w - 52.0).max(64.0), 19.0 ], copy_button).on_hover_text(format!("{} (Ctrl+C)", full_copy_label)).clicked() {
                                                self.copy_text(&s.style, "Style");
                                            }
                                            ui.add_sized([49.0, 19.0], egui::Label::new(egui::RichText::new(format!("{count}/1000")).size(9.5).color(color)));
                                        });
                                    },
                                );

                                ui.allocate_ui_with_layout(
                                    egui::vec2(exclusion_w, row_height),
                                    egui::Layout::top_down(egui::Align::Min),
                                    |ui| {
                                        egui::ScrollArea::vertical()
                                            .id_salt(("exclusion_cell_v24", s.id))
                                            .max_height(40.0)
                                            .auto_shrink([false, false])
                                            .show(ui, |ui| {
                                                ui.set_width(exclusion_w - 4.0);
                                                let response = ui.add(egui::Label::new(egui::RichText::new(&s.exclusion).size(11.5)).wrap().sense(egui::Sense::click()));
                                                if response.double_clicked() { edit_on_double_click = Some(s.id); }
                                                if response.clicked() { self.selected_id = Some(s.id); }
                                            });
                                        ui.add_space(1.0);
                                        ui.horizontal(|ui| {
                                            ui.spacing_mut().item_spacing.x = 2.0;
                                            let count = s.exclusion.chars().count();
                                            let color = if count > 1000 { egui::Color32::RED } else { ui.visuals().text_color() };
                                            let full_copy_label = SunoApp::tr(&self.settings.language, "copy_exclusion");
                                            let copy_label = if exclusion_w < 190.0 { "📋 Copier" } else { full_copy_label };
                                            let copy_button = egui::Button::new(copy_label).small();
                                            let copy_button = if is_selected {
                                                copy_button.fill(row_fill).stroke(egui::Stroke::NONE)
                                            } else {
                                                copy_button
                                            };
                                            if ui.add_sized([ (exclusion_w - 52.0).max(64.0), 19.0 ], copy_button).on_hover_text(format!("{} (Ctrl+Maj+C)", full_copy_label)).clicked() {
                                                self.copy_text(&s.exclusion, "Exclusion");
                                            }
                                            ui.add_sized([49.0, 19.0], egui::Label::new(egui::RichText::new(format!("{count}/1000")).size(9.5).color(color)));
                                        });
                                    },
                                );

                                let author_response = ui.add_sized(
                                    [author_w, row_height],
                                    egui::Label::new(egui::RichText::new(&s.author).size(11.5).color(if is_selected { ui.visuals().text_color() } else { ui.visuals().weak_text_color() })).wrap().sense(egui::Sense::click()),
                                ).on_hover_text(SunoApp::tr(&self.settings.language, "author"));
                                if author_response.clicked() { self.selected_id = Some(s.id); }
                                if author_response.double_clicked() { edit_on_double_click = Some(s.id); }
                                ui.end_row();
                            }
                        });
                });
            if let Some((id, favorite)) = favorite_toggle { self.set_favorite(id, favorite); }
            if let Some(id) = edit_on_double_click {
                self.selected_id = Some(id);
                self.open_edit();
            }
        });

        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                let (ready_icon_rect, _) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
                draw_icon(ui.painter(), ready_icon_rect.center(), "ready", ui.visuals().text_color());
                ui.label(&self.status);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(SunoApp::tr(&self.settings.language, "count").replace("{shown}", &displayed_count.to_string()).replace("{total}", &total_count.to_string()));
                });
            });
        });

        if self.import_dialog_open {
            let mut open = self.import_dialog_open;
            let import_window = egui::Window::new(SunoApp::tr(&self.settings.language, "import_mode_title"))
                .open(&mut open).anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .collapsible(false).resizable(false).default_width(390.0).show(ctx, |ui| {
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new(SunoApp::tr(&self.settings.language, "merge")).strong().color(egui::Color32::from_rgb(75, 145, 220)));
                        ui.label(SunoApp::tr(&self.settings.language, "merge_description"));
                        ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                            if modal_button(ui, &SunoApp::tr(&self.settings.language, "merge"), ModalButtonTone::Primary).clicked() {
                                self.import_dialog_open = false;
                                self.import_styles(false);
                            }
                        });
                        ui.add_space(8.0);
                        ui.separator();
                        ui.label(egui::RichText::new(SunoApp::tr(&self.settings.language, "overwrite")).strong().color(egui::Color32::from_rgb(205, 105, 65)));
                        ui.label(SunoApp::tr(&self.settings.language, "overwrite_description"));
                        ui.label(egui::RichText::new(SunoApp::tr(&self.settings.language, "overwrite_warning")).color(ui.visuals().error_fg_color));
                        ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                            if modal_button(ui, &SunoApp::tr(&self.settings.language, "overwrite"), ModalButtonTone::Danger).clicked() {
                                self.import_dialog_open = false;
                                self.import_styles(true);
                            }
                        });
                        ui.add_space(4.0);
                        ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                            if modal_button(ui, &SunoApp::tr(&self.settings.language, "cancel"), ModalButtonTone::Secondary).clicked() {
                                self.import_dialog_open = false;
                            }
                        });
                    });
                });
            if import_was_open_at_frame_start {
                if let Some(output) = &import_window {
                    let clicked_outside = ctx.input(|i| i.pointer.any_click() && i.pointer.interact_pos().map_or(false, |pos| !output.response.rect.contains(pos)));
                    if clicked_outside { open = false; }
                }
            }
            self.import_dialog_open = open && self.import_dialog_open;
        }

        if let Some((added, skipped, overwrite)) = self.import_report {
            let mut close_report = false;
            egui::Window::new(SunoApp::tr(&self.settings.language, "import_report_title"))
                .collapsible(false).resizable(false).default_width(360.0).show(ctx, |ui| {
                    let heading_key = if overwrite { "import_report_overwrite" } else { "import_report_merge" };
                    ui.heading(SunoApp::tr(&self.settings.language, heading_key));
                    ui.add_space(8.0);
                    ui.label(SunoApp::tr(&self.settings.language, "import_report_added")
                        .replace("{count}", &added.to_string()));
                    ui.label(SunoApp::tr(&self.settings.language, "import_report_skipped")
                        .replace("{count}", &skipped.to_string()));
                    ui.add_space(8.0);
                    if modal_button(ui, &SunoApp::tr(&self.settings.language, "ok"), ModalButtonTone::Primary).clicked() {
                        close_report = true;
                    }
                });
            if close_report { self.import_report = None; }
        }

        if self.manual_open {
            let mut open = self.manual_open;
            let manual_window = egui::Window::new(SunoApp::tr(&self.settings.language, "manual_title"))
                .open(&mut open)
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .collapsible(false).resizable(true).default_width(610.0).min_width(480.0)
                .show(ctx, |ui| {
                    ui.label(egui::RichText::new(SunoApp::tr(&self.settings.language, "manual_intro"))
                        .size(14.0).strong().color(egui::Color32::from_rgb(75, 145, 220)));
                    ui.add_space(8.0);
                    egui::ScrollArea::vertical().max_height(480.0).show(ui, |ui| {
                        let language = self.settings.language.as_str();
                        let sections: &[(&str, &str)] = match language {
                            "English" => &[
                                ("1. Library", "The main table contains your saved Suno styles. Select a row to use the available actions. Use All styles, Favorites, or a musical-style tag to filter the list."),
                                ("2. Add a style", "Click New. You can paste a Suno song URL and click Analyze to fill in the available information, then review and complete the fields before saving. You can also enter the information manually."),
                                ("3. Edit or delete", "Select a row, then click Edit or double-click the row. Make your changes and save. Delete asks for confirmation before permanently removing an entry."),
                                ("4. Musical styles and Lyrics tags", "Musical styles are comma-separated keywords you enter yourself; click a musical-style tag in the table to filter entries containing that tag. Lyrics tags are the bracketed tags associated with the Lyrics area."),
                                ("5. Search", "Open Search to choose which fields are searched, such as the name, Lyrics tags, musical style, or exclusion prompt. Clear the search to show matching entries without the text filter."),
                                ("6. Import and export", "Export saves your library to a JSON file. Import offers Merge, which adds entries while keeping existing ones, or Overwrite, which replaces the library. Keep a backup before overwriting."),
                                ("7. Options", "Options lets you change the interface language and display theme. Your styles are stored locally in the application's SQLite database."),
                                ("Keyboard shortcuts", "Ctrl+N: new style   •   Ctrl+E: edit selected style   •   Ctrl+F: open search   •   Delete: request deletion   •   Ctrl+C: copy Style prompt   •   Ctrl+Shift+C: copy Exclusion prompt   •   Escape: close search."),
                            ],
                            "Deutsch" => &[
                                ("1. Bibliothek", "Die Haupttabelle enthält deine gespeicherten Suno-Stile. Wähle eine Zeile aus, um Aktionen auszuführen. Verwende Alle Stile, Favoriten oder ein Musikstil-Tag zum Filtern."),
                                ("2. Stil hinzufügen", "Klicke auf Neu. Füge optional eine Suno-Song-URL ein und klicke auf Analysieren. Prüfe und ergänze die ausgefüllten Felder vor dem Speichern. Du kannst alle Angaben auch manuell eingeben."),
                                ("3. Bearbeiten oder löschen", "Wähle eine Zeile und klicke auf Bearbeiten oder doppelklicke auf die Zeile. Speichere deine Änderungen. Beim Löschen musst du die endgültige Entfernung bestätigen."),
                                ("4. Musikstil und Lyrics-Tags", "Musikstile sind selbst eingegebene, durch Kommas getrennte Stichwörter. Klicke auf ein Musikstil-Tag in der Tabelle, um passende Einträge zu filtern. Lyrics-Tags sind die Tags im Lyrics-Bereich."),
                                ("5. Suche", "Öffne Suche und wähle die zu durchsuchenden Felder: Name, Lyrics-Tags, Musikstil oder Ausschluss-Prompt. Lösche den Suchtext, um den Textfilter zu entfernen."),
                                ("6. Import und Export", "Export speichert die Bibliothek als JSON-Datei. Beim Import kannst du Zusammenführen wählen, um vorhandene Einträge zu behalten, oder Überschreiben, um die Bibliothek zu ersetzen. Erstelle vorher eine Sicherung."),
                                ("7. Optionen", "Unter Optionen kannst du Sprache und Darstellung ändern. Deine Stile werden lokal in einer SQLite-Datenbank gespeichert."),
                                ("Tastenkürzel", "Strg+N: neuer Stil • Strg+E: ausgewählten Stil bearbeiten • Strg+F: Suche öffnen • Entf: Löschen bestätigen • Strg+C: Stil-Prompt kopieren • Strg+Umschalt+C: Ausschluss-Prompt kopieren • Esc: Suche schließen."),
                            ],
                            "Español" => &[
                                ("1. Biblioteca", "La tabla principal contiene tus estilos de Suno guardados. Selecciona una fila para usar las acciones. Filtra con Todos los estilos, Favoritos o una etiqueta de estilo musical."),
                                ("2. Añadir un estilo", "Pulsa Nuevo. Puedes pegar una URL de una canción de Suno y pulsar Analizar; revisa y completa los campos antes de guardar. También puedes introducir todos los datos manualmente."),
                                ("3. Editar o eliminar", "Selecciona una fila y pulsa Editar o haz doble clic en ella. Guarda los cambios. Para eliminar una entrada, debes confirmar la eliminación definitiva."),
                                ("4. Estilos musicales y etiquetas Lyrics", "Los estilos musicales son palabras clave separadas por comas que introduces tú. Pulsa una etiqueta en la tabla para filtrar las entradas que la contienen. Las etiquetas Lyrics corresponden a la sección de letras."),
                                ("5. Búsqueda", "Abre Buscar para elegir los campos que se buscan: nombre, etiquetas Lyrics, estilo musical o exclusiones. Borra el texto de búsqueda para quitar ese filtro."),
                                ("6. Importar y exportar", "Exportar guarda la biblioteca en un archivo JSON. Importar permite Fusionar, conservando las entradas existentes, o Sobrescribir, reemplazando la biblioteca. Haz una copia de seguridad antes de sobrescribir."),
                                ("7. Opciones", "En Opciones puedes cambiar el idioma y el tema visual. Los estilos se guardan localmente en una base de datos SQLite."),
                                ("Atajos de teclado", "Ctrl+N: nuevo estilo • Ctrl+E: editar estilo seleccionado • Ctrl+F: abrir búsqueda • Supr: confirmar eliminación • Ctrl+C: copiar prompt Style • Ctrl+Mayús+C: copiar prompt Exclusion • Esc: cerrar búsqueda."),
                            ],
                            "Português" => &[
                                ("1. Biblioteca", "A tabela principal contém os estilos Suno guardados. Selecione uma linha para utilizar as ações. Filtre por Todos os estilos, Favoritos ou por uma etiqueta de estilo musical."),
                                ("2. Adicionar um estilo", "Clique em Novo. Pode colar um URL de uma música Suno e clicar em Analisar; reveja e complete os campos antes de guardar. Também pode introduzir os dados manualmente."),
                                ("3. Editar ou eliminar", "Selecione uma linha e clique em Editar ou faça duplo clique na linha. Guarde as alterações. A eliminação definitiva exige confirmação."),
                                ("4. Estilos musicais e tags Lyrics", "Os estilos musicais são palavras-chave separadas por vírgulas, introduzidas por si. Clique numa etiqueta da tabela para filtrar entradas que a contenham. As tags Lyrics correspondem à secção de letras."),
                                ("5. Pesquisa", "Abra Pesquisar para escolher os campos pesquisados: nome, tags Lyrics, estilo musical ou exclusões. Apague o texto de pesquisa para remover esse filtro."),
                                ("6. Importar e exportar", "Exportar guarda a biblioteca num ficheiro JSON. Importar permite Fundir, mantendo as entradas existentes, ou Substituir, trocando a biblioteca. Faça uma cópia de segurança antes de substituir."),
                                ("7. Opções", "Em Opções pode alterar o idioma e o tema visual. Os estilos são guardados localmente numa base de dados SQLite."),
                                ("Atalhos de teclado", "Ctrl+N: novo estilo • Ctrl+E: editar estilo selecionado • Ctrl+F: abrir pesquisa • Delete: confirmar eliminação • Ctrl+C: copiar prompt Style • Ctrl+Shift+C: copiar prompt Exclusion • Esc: fechar pesquisa."),
                            ],
                            "中文" => &[
                                ("1. 样式库", "主表格显示已保存的 Suno 样式。单击一行可选择条目并使用相关操作。可通过“所有样式”“收藏”或音乐风格标签筛选。"),
                                ("2. 添加样式", "点击“新建”。可以粘贴 Suno 歌曲链接并点击“分析”，然后检查并补充信息再保存；也可以手动填写所有字段。"),
                                ("3. 编辑或删除", "选择一行后点击“编辑”，也可以双击该行。修改后保存。删除条目之前需要确认永久删除。"),
                                ("4. 音乐风格与 Lyrics 标签", "音乐风格由你手动填写，多个关键词用逗号分隔。点击表格中的音乐风格标签，可筛选包含该标签的条目。Lyrics 标签来自歌词区域。"),
                                ("5. 搜索", "打开搜索并选择搜索字段，例如名称、Lyrics 标签、音乐风格或排除提示词。清空搜索文本即可取消文本筛选。"),
                                ("6. 导入与导出", "导出会将样式库保存为 JSON 文件。导入时可选择“合并”以保留已有条目，或选择“覆盖”以替换样式库。覆盖前请先备份。"),
                                ("7. 选项", "在选项中可以更改界面语言和显示主题。样式保存在本地 SQLite 数据库中。"),
                                ("键盘快捷键", "Ctrl+N：新建样式 • Ctrl+E：编辑所选样式 • Ctrl+F：打开搜索 • Delete：请求删除 • Ctrl+C：复制 Style 提示词 • Ctrl+Shift+C：复制 Exclusion 提示词 • Esc：关闭搜索。"),
                            ],
                            "العربية" => &[
                                ("١. المكتبة", "يعرض الجدول الرئيسي أنماط Suno المحفوظة. حدّد صفًا لاستخدام الإجراءات المتاحة. يمكنك التصفية حسب كل الأنماط أو المفضلة أو وسم النمط الموسيقي."),
                                ("٢. إضافة نمط", "اضغط على جديد. يمكنك لصق رابط أغنية Suno والضغط على تحليل، ثم مراجعة الحقول وإكمالها قبل الحفظ. ويمكنك أيضًا إدخال المعلومات يدويًا."),
                                ("٣. التعديل أو الحذف", "حدّد صفًا ثم اضغط تعديل أو انقر عليه مرتين. احفظ التغييرات. يتطلب الحذف تأكيد الإزالة النهائية."),
                                ("٤. الأنماط الموسيقية ووسوم Lyrics", "الأنماط الموسيقية كلمات مفتاحية تفصل بينها فواصل وتدخلها يدويًا. انقر على وسم في الجدول لتصفية الإدخالات التي تحتوي عليه. وسوم Lyrics تخص قسم الكلمات."),
                                ("٥. البحث", "افتح البحث واختر الحقول التي تريد البحث فيها، مثل الاسم أو وسوم Lyrics أو النمط الموسيقي أو الاستثناءات. امسح نص البحث لإزالة هذا المرشح."),
                                ("٦. الاستيراد والتصدير", "يحفظ التصدير المكتبة في ملف JSON. يتيح الاستيراد الدمج مع الاحتفاظ بالإدخالات الحالية أو الاستبدال الذي يستبدل المكتبة. أنشئ نسخة احتياطية قبل الاستبدال."),
                                ("٧. الخيارات", "يمكنك تغيير اللغة ومظهر الواجهة من الخيارات. تُحفظ الأنماط محليًا في قاعدة بيانات SQLite."),
                                ("اختصارات لوحة المفاتيح", "Ctrl+N: نمط جديد • Ctrl+E: تعديل النمط المحدد • Ctrl+F: فتح البحث • Delete: طلب الحذف • Ctrl+C: نسخ مطالبة Style • Ctrl+Shift+C: نسخ مطالبة Exclusion • Esc: إغلاق البحث."),
                            ],
                            _ => &[
                                ("1. La bibliothèque", "Le tableau principal contient les styles Suno enregistrés. Cliquez sur une ligne pour la sélectionner. Utilisez « Tous les styles », « Favoris » ou un tag de style musical pour filtrer la liste."),
                                ("2. Ajouter un style", "Cliquez sur « Nouveau ». Vous pouvez coller un lien de chanson Suno puis cliquer sur « Analyser » pour préremplir les informations disponibles. Relisez et complétez les champs avant d’enregistrer ; la saisie manuelle reste possible."),
                                ("3. Modifier ou supprimer", "Sélectionnez une ligne puis cliquez sur « Édition », ou double-cliquez sur la ligne. Modifiez les champs puis enregistrez. La suppression demande une confirmation avant d’effacer définitivement l’entrée."),
                                ("4. Style musical et tags Lyrics", "Le champ « Style musical » contient vos mots-clés, séparés par des virgules. Cliquez sur un tag dans la colonne pour filtrer les entrées qui le contiennent. Les tags Lyrics correspondent aux balises de la zone Lyrics."),
                                ("5. Recherche", "Ouvrez « Rechercher » et choisissez les champs à parcourir : nom, tags Lyrics, style musical ou exclusions. Vous pouvez vider la recherche pour retirer le filtre textuel."),
                                ("6. Importer et exporter", "« Exporter » enregistre la bibliothèque dans un fichier JSON. À l’import, « Fusionner » conserve les entrées existantes et ajoute les nouvelles ; « Écraser » remplace la bibliothèque. Faites une sauvegarde avant un écrasement."),
                                ("7. Options", "Les Options permettent de choisir la langue de l’interface et le thème clair, sombre ou système. Les styles sont conservés localement dans une base de données SQLite."),
                                ("Raccourcis clavier", "Ctrl+N : nouveau style • Ctrl+E : modifier le style sélectionné • Ctrl+F : ouvrir la recherche • Suppr : demander la suppression • Ctrl+C : copier le prompt Style • Ctrl+Maj+C : copier le prompt Exclusion • Échap : fermer la recherche."),
                            ],
                        };
                        for (heading, body) in sections {
                            ui.add_space(5.0);
                            ui.label(egui::RichText::new(*heading).strong().size(14.0));
                            ui.label(*body);
                            ui.add_space(3.0);
                            ui.separator();
                        }
                    });
                    ui.add_space(8.0);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if modal_button(ui, &SunoApp::tr(&self.settings.language, "close"), ModalButtonTone::Primary).clicked() {
                            self.manual_open = false;
                        }
                    });
                });
            if manual_was_open_at_frame_start {
                if let Some(output) = &manual_window {
                    let clicked_outside = ctx.input(|i| i.pointer.any_click() && i.pointer.interact_pos().map_or(false, |pos| !output.response.rect.contains(pos)));
                    if clicked_outside { open = false; }
                }
            }
            self.manual_open = open && self.manual_open;
        }

        if self.settings_open {
            let mut open = self.settings_open;
            let settings_title = if self.settings_about { SunoApp::tr(&self.settings.language, "about") } else { SunoApp::tr(&self.settings.language, "options") };
            let settings_window = egui::Window::new(settings_title)
                .open(&mut open).anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .collapsible(false).resizable(false).default_width(360.0).show(ctx, |ui| {
                if self.settings_about {
                    ui.add_space(12.0);
                    ui.vertical_centered(|ui| {
                        ui.heading(egui::RichText::new("Suno Style Manager").strong().size(22.0));
                        ui.add_space(10.0);
                        ui.label(SunoApp::tr(&self.settings.language, "app_version"));
                        ui.add_space(6.0);
                        ui.label(SunoApp::tr(&self.settings.language, "creator"));
                    });
                    ui.add_space(18.0);
                    ui.separator();
                    if modal_button(ui, SunoApp::tr(&self.settings.language, "back_to_options"), ModalButtonTone::Secondary).clicked() {
                        self.settings_about = false;
                    }
                } else {
                ui.label(egui::RichText::new(SunoApp::tr(&self.settings.language, "preferences_title")).size(17.0).strong().color(egui::Color32::from_rgb(75, 145, 220)));
                ui.separator();
                ui.add_space(8.0);
                ui.label(SunoApp::language_label(&self.settings.language));
                egui::ComboBox::from_id_salt("app_language")
                    .selected_text(&self.settings.language)
                    .show_ui(ui, |ui| {
                        for language in ["Français", "English", "Deutsch", "Español", "Português", "中文", "العربية"] {
                            ui.horizontal(|ui| {
                                draw_flag(ui, language);
                                ui.selectable_value(&mut self.settings.language, language.to_string(), language);
                            });
                        }
                    });
                ui.add_space(10.0);
                ui.label(SunoApp::tr(&self.settings.language, "theme"));
                let theme_key = match self.settings.theme.as_str() { "Mode sombre" => "dark", "Mode clair" => "light", _ => "system" };
                egui::ComboBox::from_id_salt("app_theme")
                    .selected_text(SunoApp::tr(&self.settings.language, theme_key))
                    .show_ui(ui, |ui| {
                        for (theme, key) in [("Mode sombre", "dark"), ("Mode clair", "light"), ("Même que l’OS", "system")] {
                            ui.selectable_value(&mut self.settings.theme, theme.to_string(), SunoApp::tr(&self.settings.language, key));
                        }
                    });
                ui.add_space(12.0);
                ui.checkbox(&mut self.settings.add_suno_link, SunoApp::tr(&self.settings.language, "add_suno_link"));
                ui.add_space(4.0);
                if modal_button(ui, SunoApp::tr(&self.settings.language, "delete_suno_links"), ModalButtonTone::Danger).clicked() {
                    self.delete_suno_links();
                }
                ui.add_space(12.0);
                ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                    ui.horizontal(|ui| {
                        if modal_button(ui, &SunoApp::tr(&self.settings.language, "save"), ModalButtonTone::Primary).clicked() { self.save_settings(); self.settings_open = false; }
                        if modal_button(ui, &SunoApp::tr(&self.settings.language, "cancel"), ModalButtonTone::Secondary).clicked() { self.settings = Self::load_settings(); self.settings_open = false; }
                    });
                });
                ui.add_space(4.0);
                ui.label(egui::RichText::new(SunoApp::tr(&self.settings.language, "prefs_saved")).small());
                ui.add_space(8.0);
                if modal_button(ui, SunoApp::tr(&self.settings.language, "about"), ModalButtonTone::Secondary).clicked() {
                    self.settings_about = true;
                }
                }
            });
            if settings_was_open_at_frame_start {
                if let Some(output) = &settings_window {
                    let clicked_outside = ctx.input(|i| i.pointer.any_click() && i.pointer.interact_pos().map_or(false, |pos| !output.response.rect.contains(pos)));
                    if clicked_outside { open = false; }
                }
            }
            self.settings_open = open && self.settings_open;
        }

        if self.editor_open {
            let title = if self.editor_mode == EditorMode::New { SunoApp::tr(&self.settings.language, "new_title") } else { SunoApp::tr(&self.settings.language, "editor_title") };
            let mut open = self.editor_open;
            let editor_window = egui::Window::new(title).open(&mut open)
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .collapsible(false).resizable(true).default_width(620.0).show(ctx, |ui| {
                ui.label(egui::RichText::new(if self.editor_mode == EditorMode::New { SunoApp::tr(&self.settings.language, "new_title") } else { SunoApp::tr(&self.settings.language, "editor_title") }).size(17.0).strong().color(egui::Color32::from_rgb(75, 145, 220)));
                ui.separator();
                if self.editor_mode == EditorMode::New {
                    ui.label(egui::RichText::new(SunoApp::tr(&self.settings.language, "song_link")).strong());
                    ui.horizontal(|ui| {
                        let url_response = ui.add(egui::TextEdit::singleline(&mut self.song_url).desired_width(470.0).hint_text("https://suno.com/song/..."));
                        if url_response.changed() {
                            self.suno_analysis_checked = false;
                        }
                        if modal_button(ui, &SunoApp::tr(&self.settings.language, "analyze"), ModalButtonTone::Secondary).clicked() {
                            match parse_suno_song(&self.song_url) {
                                Ok(parsed) => {
                                    self.draft.name = parsed.title;
                                    self.draft.tags = parsed.tags;
                                    self.draft.style = parsed.style;
                                    self.draft.exclusion = parsed.exclusion;
                                    self.draft.author = parsed.author;
                                    self.draft.suno_url = if self.settings.add_suno_link { self.song_url.trim().to_string() } else { String::new() };
                                    self.suno_analysis_checked = true;
                                    if is_duplicate_suno_entry(&self.styles, &self.draft.name, &self.song_url) {
                                        self.status = SunoApp::tr(&self.settings.language, "duplicate_suno_entry").to_string();
                                    } else {
                                        self.status = SunoApp::tr(&self.settings.language, "suno_analyzed").into();
                                    }
                                }
                                Err(e) => {
                                    self.suno_analysis_checked = false;
                                    self.status = format!("Analyse Suno impossible : {e}");
                                }
                            }
                        }
                    });
                    if self.suno_analysis_checked && is_duplicate_suno_entry(&self.styles, &self.draft.name, &self.song_url) {
                        ui.label(egui::RichText::new(SunoApp::tr(&self.settings.language, "duplicate_suno_entry"))
                            .color(egui::Color32::RED));
                    }
                    ui.add_space(8.0);
                }
                ui.label(SunoApp::tr(&self.settings.language, "name_label")); ui.add(egui::TextEdit::singleline(&mut self.draft.name).desired_width(560.0));
                ui.add_space(6.0);
                ui.label(SunoApp::tr(&self.settings.language, "music_style_label"));
                ui.add(egui::TextEdit::singleline(&mut self.draft.music_style)
                    .desired_width(560.0)
                    .hint_text("folk, acoustique, mélancolique"));
                ui.add_space(6.0);
                ui.label(SunoApp::tr(&self.settings.language, "tags_label"));
                ui.add(egui::TextEdit::multiline(&mut self.draft.tags).desired_width(560.0).desired_rows(4));
                ui.add_space(6.0);
                ui.label(SunoApp::tr(&self.settings.language, "style_label"));
                ui.add(egui::TextEdit::multiline(&mut self.draft.style).desired_width(560.0).desired_rows(7).hint_text(SunoApp::tr(&self.settings.language, "style_hint")));
                ui.add_space(6.0);
                ui.label(SunoApp::tr(&self.settings.language, "exclusion_label"));
                ui.add(egui::TextEdit::multiline(&mut self.draft.exclusion).desired_width(560.0).desired_rows(4).hint_text(SunoApp::tr(&self.settings.language, "exclusion_hint")));
                ui.add_space(10.0);
                ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                    ui.horizontal(|ui| {
                        if modal_button(ui, &SunoApp::tr(&self.settings.language, "save"), ModalButtonTone::Primary).clicked() { self.save_draft(); }
                        if modal_button(ui, &SunoApp::tr(&self.settings.language, "cancel"), ModalButtonTone::Secondary).clicked() { self.editor_open = false; }
                    });
                });
            });
            if editor_was_open_at_frame_start {
                if let Some(output) = &editor_window {
                    let clicked_outside = ctx.input(|i| i.pointer.any_click() && i.pointer.interact_pos().map_or(false, |pos| !output.response.rect.contains(pos)));
                    if clicked_outside { open = false; }
                }
            }
            self.editor_open = open && self.editor_open;
        }

        if self.confirm_delete {
            egui::Window::new(SunoApp::tr(&self.settings.language, "delete_title")).collapsible(false).resizable(false).show(ctx, |ui| {
                let name = self.styles.iter().find(|s| Some(s.id) == self.selected_id).map(|s| s.name.as_str()).unwrap_or("ce style");
                ui.label(SunoApp::tr(&self.settings.language, "delete_prompt").replace("{name}", name));
                ui.horizontal(|ui| {
                    if modal_button(ui, &SunoApp::tr(&self.settings.language, "delete_confirm"), ModalButtonTone::Danger).clicked() { self.delete_selected(); }
                    if modal_button(ui, &SunoApp::tr(&self.settings.language, "cancel"), ModalButtonTone::Secondary).clicked() { self.confirm_delete = false; }
                });
            });
        }
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions { viewport: egui::ViewportBuilder::default().with_inner_size([1180.0, 720.0]).with_min_inner_size([800.0, 500.0]), ..Default::default() };
    eframe::run_native("Gestionnaire de Styles Suno", options, Box::new(|cc| {
        let mut fonts = egui::FontDefinitions::default();
        fonts.font_data.insert(
            "noto_sans_sc_ui".to_owned(),
            egui::FontData::from_static(include_bytes!("../assets/NotoSansSC-UI.ttf")).into(),
        );
        fonts.font_data.insert(
            "noto_sans_arabic_ui".to_owned(),
            egui::FontData::from_static(include_bytes!("../assets/NotoSansArabicUI-Regular.ttf")).into(),
        );
        let proportional = fonts.families.entry(egui::FontFamily::Proportional).or_default();
        proportional.push("noto_sans_sc_ui".to_owned());
        proportional.push("noto_sans_arabic_ui".to_owned());
        let monospace = fonts.families.entry(egui::FontFamily::Monospace).or_default();
        monospace.push("noto_sans_sc_ui".to_owned());
        monospace.push("noto_sans_arabic_ui".to_owned());
        cc.egui_ctx.set_fonts(fonts);
        Ok(Box::new(SunoApp::new()))
    }))
}


#[cfg(test)]
mod reliability_tests {
    use super::*;

    #[test]
    fn duplicate_suno_entry_detects_same_name_case_insensitively() {
        let styles = vec![SunoStyle { name: "Balade Nocturne".into(), ..SunoStyle::default() }];
        assert!(is_duplicate_suno_entry(&styles, " balade nocturne ", "https://suno.com/song/other"));
    }

    #[test]
    fn duplicate_suno_entry_detects_same_url_even_when_name_differs() {
        let styles = vec![SunoStyle { name: "Premier titre".into(), suno_url: "https://suno.com/song/abc/".into(), ..SunoStyle::default() }];
        assert!(is_duplicate_suno_entry(&styles, "Autre titre", "HTTPS://SUNO.COM/song/abc?share=1"));
    }

    #[test]
    fn different_suno_name_and_url_are_not_duplicates() {
        let styles = vec![SunoStyle { name: "Premier titre".into(), suno_url: "https://suno.com/song/abc".into(), ..SunoStyle::default() }];
        assert!(!is_duplicate_suno_entry(&styles, "Autre titre", "https://suno.com/song/xyz"));
    }
    use super::*;

    #[test]
    fn legacy_settings_without_suno_link_option_default_to_disabled() {
        let settings: AppSettings = serde_json::from_str(
            r#"{"language":"Français","theme":"Même que l’OS"}"#
        ).expect("les anciens paramètres restent compatibles");
        assert!(!settings.add_suno_link);
        assert_eq!(settings.language, "Français");
    }

    #[test]
    fn default_settings_do_not_store_suno_links() {
        assert!(!AppSettings::default().add_suno_link);
    }

    #[test]
    fn portable_style_round_trip_preserves_all_business_fields() {
        let original = PortableStyle {
            name: "  Folk nocturne  ".into(),
            tags: "[Verse], [Chorus]".into(),
            music_style: "folk, acoustique, mélancolique".into(),
            style: "warm acoustic guitar, close vocal".into(),
            exclusion: "EDM, heavy distortion".into(),
            author: "artiste-demo".into(),
            suno_url: "https://suno.com/song/example".into(),
            favorite: true,
        };
        let json = serde_json::to_string(&original).expect("sérialisation JSON");
        let restored: PortableStyle = serde_json::from_str(&json).expect("désérialisation JSON");
        assert_eq!(restored.name, original.name);
        assert_eq!(restored.tags, original.tags);
        assert_eq!(restored.music_style, original.music_style);
        assert_eq!(restored.style, original.style);
        assert_eq!(restored.exclusion, original.exclusion);
        assert_eq!(restored.author, original.author);
        assert_eq!(restored.suno_url, original.suno_url);
        assert_eq!(restored.favorite, original.favorite);
    }

    #[test]
    fn legacy_json_without_new_fields_remains_importable() {
        let restored: PortableStyle = serde_json::from_str(
            r#"{"name":"Ancien style","tags":"[Verse]","style":"piano"}"#
        ).expect("format historique compatible");
        assert_eq!(restored.name, "Ancien style");
        assert_eq!(restored.tags, "[Verse]");
        assert_eq!(restored.style, "piano");
        assert_eq!(restored.exclusion, "");
        assert_eq!(restored.music_style, "");
        assert_eq!(restored.author, "");
        assert_eq!(restored.suno_url, "");
        assert!(!restored.favorite);
    }

    #[test]
    fn malformed_json_is_rejected() {
        let result = serde_json::from_str::<Vec<PortableStyle>>(r#"[{"name":"Style cassé",]"#);
        assert!(result.is_err());
    }

    #[test]
    fn music_style_filter_matches_complete_tags_case_insensitively() {
        assert!(music_style_has_tag("folk, acoustique, mélancolique", "Acoustique"));
        assert!(music_style_has_tag("  Rock , Pop  ", "rock"));
        assert!(!music_style_has_tag("indie rock, pop", "rock"));
        assert!(!music_style_has_tag("folk, pop", " "));
    }

    #[test]
    fn import_name_validation_skips_empty_and_case_insensitive_duplicates() {
        let mut names = HashSet::new();
        let first = normalized_name_key("  Ballade  ").expect("nom valide");
        assert!(names.insert(first));
        let duplicate = normalized_name_key("ballade").expect("nom valide");
        assert!(!names.insert(duplicate));
        assert!(normalized_name_key("   ").is_none());
        let distinct = normalized_name_key("Rock").expect("nom valide");
        assert!(names.insert(distinct));
    }
}
