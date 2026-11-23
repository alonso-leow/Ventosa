//! The node gallery: step nodes a user can compose into a new graph.
//! Sources: the steps of every registered workspace and of the installed
//! templates ("existing"), plus a set of common production steps. A composed
//! graph runs on the engine's generic flow: steps in order, each one finished
//! with `ventosa-graph write step-done` (AI) or `human-done` (human).

use crate::config::Workspace;
use crate::graph;
use serde_json::{json, Value};
use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub struct NodeDef {
    pub id: String,
    pub owner: String,
    pub need: String,
    pub skill: String,
    /// Where it came from: a workspace or template name, "common", or a plugin name.
    pub source: String,
    /// Extra step fields written on add (`kind`, `app`, `per`, `path`…); `Null` = none.
    pub extras: Value,
}

/// "script_draft" → "Script Draft", "audio_fx" → "Audio FX": every word starts with a capital.
pub fn display_name(id: &str) -> String {
    id.split(['_', '-', ' '])
        .filter(|w| !w.is_empty())
        .map(|w| {
            if w.eq_ignore_ascii_case("fx") || w.eq_ignore_ascii_case("ai") || w.eq_ignore_ascii_case("id") {
                return w.to_uppercase();
            }
            let mut c = w.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Steps that belong to the real app workflow, not to a graph: the label app and the folder node stand in for them.
pub const NOT_GRAPH_STEPS: [&str; 2] = ["publish", "upload"];

fn common_def(id: &str, owner: &str, need: &str, body: &str) -> NodeDef {
    let skill = format!("# Node skill: {id} [{}]\n{need}\n\n{body}\n", if owner == "ai" { "AI" } else { "human" });
    NodeDef { id: id.into(), owner: owner.into(), need: need.into(), skill, source: "common".into(), extras: Value::Null }
}

fn trim_clip() -> NodeDef {
    let mut n = common_def("trim_clip", "ai", "trim one clip to its in and out points (runs once per clip)", "**Runs per clip:** one run for every row of `assets/edits/<ID>-clips.md`; the run's clip number is in the item's detail. **Do:** cut that clip from the edited media with `ffmpeg -ss <in> -to <out> -i <edited> -c:v libx264 -c:a aac assets/clips/<ID>-<n>.mp4` (re-encode: cuts on exact phrase times, not on keyframes), check the length matches out − in within 0.1 s, and add a line `n · file · length` to `assets/clips/<ID>-index.md`. Never overwrite a trimmed clip a human approved.");
    n.extras = json!({"per": "clip"});
    n
}

fn label_media() -> NodeDef {
    let mut n = common_def("label_media", "human", "label the final media: approve it or send it back (Media Labelling app)", "**Human does:** opens the Media Labelling app (Ventosa ▸ apps, or the node's button), watches each final version and approves it or sends it back with a note. Approved versions are what gets published; a send-back reopens the step named in the note. Marks done when every version has a verdict.");
    n.extras = json!({"app": "media-labelling"});
    n
}

fn folder_node() -> NodeDef {
    let mut n = common_def("folder", "human", "open a folder or file in the Files view with the file selected", "**Human does:** presses Open in Files on the node: the Files view opens in the node's folder with the file selected (video and image previews included). The node's `path` is relative to the workspace root and may use `<ID>` for the subject. Marks done when the file is checked.");
    n.extras = json!({"kind": "folder", "path": "assets/edits/<ID>.mp4"});
    n
}

/// Common production steps, ready to drop into any graph.
pub fn common() -> Vec<NodeDef> {
    vec![
        common_def("pool", "ai", "gather a pool of candidates for the subject: angles, references, images, clips, songs, facts", "**Do:** write `research/<ID>-pool.md`: 6–10 candidates of the kind this graph needs, one line each (`candidate · type · why it matters · where it comes from`). Types may mix: angles and theses (when the graph needs a thesis, give it as an angle: one sentence, its objectives and the counter-argument), reference images or footage, songs, facts, formats. No final choice yet: a human or a later node picks."),
        common_def("pick", "human", "choose which options get a full draft", "**Human does:** reads the options, keeps the ones to draft, then marks done in Ventosa ▸ to-do."),
        common_def("voiceover", "ai", "generate the voiceover audio for the script", "**Do:** render one audio file per paragraph to `assets/voice/<ID>-<n>.wav` with the configured voice preset (keys only in env vars) and list them in `assets/voice/<ID>.json`. Note `skipped` when the piece is on camera only."),
        common_def("create_motion", "ai", "create the motion the cut needs: clips, animations, generated video", "**Do:** for each beat that needs a picture, create or pick a clip into `assets/inserts/<ID>-<beat>.<mp4|png>` and map beat → file in `assets/inserts/<ID>-sources.md`; list beats with no source."),
        common_def("organize_audio_fx", "ai", "list, obtain or generate, and place the audio effects", "**Do:** 1. List the cues the piece needs (music bed, risers, hits, ambience, ducking points) in `assets/audio/<ID>-fx.md`, one row per cue (`cue · beat · mood · length`). 2. Obtain each one from a licensed library or generate it (keys only in env vars); save under `assets/audio/fx/<ID>-<cue>.wav`. 3. Place: write the folder and file for each cue in the list (`path · licence or generator`). Licensed or generated audio only; ask with decision_ask when a licence is unclear."),
        common_def("mount_audio_fx", "ai", "mount the audio effects on the timeline with exact times", "**Do:** read `assets/audio/<ID>-fx.md` and the transcript's phrase times (`assets/edits/<ID>.json`). Write `assets/audio/<ID>-mount.json`: per cue `{file, at, duration, gain_db, duck_speech_db, fade_in, fade_out}` with `at` aligned to a phrase start or end. Loudness target −14 LUFS. Do not touch the edited media: only the mount list."),
        common_def("research", "ai", "collect facts and sources for the subject", "**Do:** read the title and notes in `labelling/labels.json`, search only the sources `tone.md` allows, and write `research/<ID>.md`: claims with a source each, open questions last."),
        common_def("outline", "ai", "write a beat outline from the research", "**Do:** write `scripts/<ID>/outline.md`: hook, three acts, call to action; one line per beat with the claim it makes and its source id."),
        common_def("script_draft", "ai", "write the full script from the outline", "**Do:** write `scripts/<ID>/draft.md` in the voice of `tone.md`, speakable sentences, one beat per paragraph, chapter markers as `**Chapter: name.**`."),
        common_def("review", "human", "read the work and approve it or send it back", "**Human does:** reads the latest output for the subject, approves (mark done in Ventosa ▸ to-do) or sends it back (`ventosa-graph write reopen <ID> <step>`)."),
        common_def("record", "human", "record the script (teleprompter: camera + mic or mic only)", "**Where:** `ventosa-teleprompter`. Takes are saved to `assets/camera/`. Mark done in Ventosa ▸ to-do when every line is kept."),
        common_def("transcript", "ai", "transcribe the edited media with the exact time of every phrase", "**Do:** use a forced-alignment tool that returns start and end per phrase, never an estimate: `whisperx <file> --model large-v3 --output_format json --output_dir assets/edits` (Connections ▸ WhisperX). From its output write `assets/edits/<ID>.json` (`[{\"start\": s, \"end\": s, \"speaker\": \"…\", \"text\": \"…\"}]`, seconds with 2 decimals), then `<ID>.txt` and `<ID>.vtt` from it. Keep speaker names; fix names from `research/`. A phrase without a time is an error: re-run the tool."),
        common_def("chapters", "ai", "split the piece into titled chapters at exact phrase times", "**Do:** read `assets/edits/<ID>.json` (phrase times). Cut where the subject changes: ≥3 chapters, each ≥10 s, each starting on a phrase start. Write `assets/edits/<ID>-chapters.txt` (`mm:ss Title`, titles ≤5 words, no spoilers) and `<ID>-chapters.json` (Podcasting 2.0: `{\"version\":\"1.2.0\",\"chapters\":[{\"startTime\":s,\"title\":\"…\"}]}`)."),
        common_def("thumbnail", "ai", "propose thumbnail or cover concepts", "**Do:** write `assets/edits/<ID>-thumbnails.md`: 3 concepts, each with the text on it (≤4 words), the image, and why it makes people click."),
        common_def("clips", "ai", "pick short clips for reels and shorts", "**Do:** from the transcript, list 3–6 clips in `assets/edits/<ID>-clips.md`, one numbered row each: `n · in · out · hook line · caption`. Times are phrase starts and ends from `assets/edits/<ID>.json`. The next node (Trim Clip) runs once for every row."),
        trim_clip(),
        common_def("translate", "ai", "translate script and subtitles", "**Do:** translate the final script and the `.vtt` into the languages listed in `tone.md`; files next to the originals with a language suffix."),
        common_def("social_posts", "ai", "write posts for each channel", "**Do:** write `assets/edits/<ID>-posts.md`: one post per channel listed in `tone.md`, with the link placeholder and the best clip."),
        common_def("newsletter", "ai", "write the newsletter issue", "**Do:** write `assets/edits/<ID>-newsletter.md`: subject line, three paragraphs, the link, one question for readers."),
        common_def("simple_visualizer", "ai", "render a simple visualizer video for a song", "**For:** an independent musician who exported a song and wants a video to upload.\n**Do:** find the song in `assets/audio/` (and its `.mid`, if any), then call the `visualizer_render` tool (or `ventosa-graph visualizer --audio … --midi … --title … --artist … --line \"Album · Year\" --palette \"#c1,#c2,#ground\" --out assets/renders/<ID>-visualizer.mp4`). Notes from MIDI scroll as a piano roll coloured by pitch along the palette; without MIDI the heard notes show as a constant-Q spectrum. Title, artist and two lines are placeholders: fill them from `labelling/labels.json` or ask the human (decision_ask) when unknown. Render a 10 s preview first (`--seconds 10`), then the full song."),
        common_def("schedule_publication", "human", "schedule the publication (optional: skip when it is published right away)", "**Human does:** schedules the upload and the posts on each platform (date, time, channel), writes the plan in `assets/edits/<ID>-schedule.md`, then marks done in Ventosa ▸ to-do. Optional: mark done at once when nothing is scheduled."),
        label_media(),
        folder_node(),
    ]
}

/// The steps of a graph folder (a workspace or a template root).
pub fn from_root(root: &Path, source: &str) -> Vec<NodeDef> {
    let Ok(text) = std::fs::read_to_string(root.join("pipeline/skill-graph/steps.json")) else { return vec![] };
    let Ok(g) = serde_json::from_str::<Value>(&text) else { return vec![] };
    g["steps"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|s| s["id"] != "done" && !NOT_GRAPH_STEPS.iter().any(|n| s["id"] == *n))
        .map(|s| {
            let skill = s["skill"].as_str().map(|p| std::fs::read_to_string(root.join("pipeline/skill-graph").join(p)).unwrap_or_default()).unwrap_or_default();
            NodeDef {
                id: s["id"].as_str().unwrap_or("").into(),
                owner: s["owner"].as_str().unwrap_or("human").into(),
                need: s["need"].as_str().unwrap_or("").into(),
                skill,
                source: source.into(),
                extras: Value::Null,
            }
        })
        .collect()
}

/// Everything the gallery offers: existing steps first (workspaces, then templates), then common ones. Unique by (id, source).
pub fn all(workspaces: &[Workspace]) -> Vec<NodeDef> {
    let mut out = vec![];
    for ws in workspaces {
        out.extend(from_root(&graph::root(ws), &ws.name));
    }
    for t in ["media", "podcast"] {
        out.extend(from_root(&graph::runtime_dir().join("templates").join(t), t));
    }
    out.extend(common());
    out.extend(crate::plugin::nodes());
    let mut seen = std::collections::HashSet::new();
    out.retain(|n| seen.insert((n.id.clone(), n.source.clone())));
    out
}

/// A template's graph as a starting list.
pub fn preset(name: &str) -> Vec<NodeDef> {
    from_root(&graph::runtime_dir().join("templates").join(name), name)
}

/// Write a composed graph into a workspace and switch it to the generic flow.
pub fn write_graph(root: &Path, name: &str, nodes: &[NodeDef]) -> Result<(), String> {
    let dir = root.join("pipeline/skill-graph");
    let nodes_dir = dir.join("nodes");
    std::fs::create_dir_all(&nodes_dir).map_err(|e| e.to_string())?;
    if let Ok(rd) = std::fs::read_dir(&nodes_dir) {
        for e in rd.flatten() {
            let _ = std::fs::remove_file(e.path());
        }
    }
    let mut ids = std::collections::HashSet::new();
    let mut steps = vec![];
    for n in nodes {
        let mut id = n.id.clone();
        let mut k = 2;
        while !ids.insert(id.clone()) {
            id = format!("{}_{k}", n.id);
            k += 1;
        }
        let ai = n.owner == "ai";
        let finish = if ai {
            format!("\n**Finish (generic flow):** `VENTOSA_AGENT=<you> ventosa-graph attach {id} <ID>` before working, then `ventosa-graph write step-done <ID> {id} \"what you wrote\"`.\n")
        } else {
            format!("\n**Finish (generic flow):** the human marks it done in Ventosa ▸ to-do (or `ventosa-graph write human-done <ID> {id}`).\n")
        };
        std::fs::write(nodes_dir.join(format!("{id}.md")), format!("{}{finish}", n.skill.trim_end())).map_err(|e| e.to_string())?;
        steps.push(json!({
            "id": id, "owner": n.owner, "need": n.need, "skill": format!("nodes/{id}.md"),
            "command": if ai { "step-done" } else { "" },
            "reads": ["K-tone", "K-meta", "K-refs", "F-labels"], "writes": [],
            "done_when": "a step.done event for this step",
        }));
        if let (Some(o), Some(st)) = (n.extras.as_object(), steps.last_mut()) {
            for (k, v) in o.iter().filter(|(k, _)| crate::graph::EXTRA_FIELDS.contains(&k.as_str())) {
                st[k.as_str()] = v.clone();
            }
        }
    }
    steps.push(json!({"id": "done", "owner": "-", "need": "", "done_when": ""}));
    let g = json!({
        "name": format!("{name}-skill-graph"),
        "elevate": graph::DEFAULT_ELEVATE,
        "rule": "Every AI agent must attach to exactly one step node for one subject (VENTOSA_AGENT=<name> ventosa-graph attach <step> <ID>) before doing pipeline work, follow that node's skill, and finish with ventosa-graph write step-done <ID> <step>.",
        "steps": steps,
        "knowledge": [
            {"id": "K-tone", "path": "tone.md", "what": "voice, structure and content rules"},
            {"id": "K-meta", "path": "meta/meta-script.md", "what": "true source in the host's voice"},
            {"id": "K-refs", "path": "research/codebase-refs.md", "what": "facts the AI may quote"},
        ],
        "files": [{"id": "F-labels", "path": "labelling/labels.json"}],
        "apps": [],
    });
    std::fs::write(dir.join("steps.json"), serde_json::to_string_pretty(&g).unwrap_or_default() + "\n").map_err(|e| e.to_string())?;
    let wj = root.join("workspace.json");
    let mut w: Value = std::fs::read_to_string(&wj).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_else(|| json!({}));
    w["flow"] = json!("generic");
    std::fs::write(&wj, serde_json::to_string_pretty(&w).unwrap_or_default() + "\n").map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gallery_has_the_new_nodes_and_no_thesis_publish_upload() {
        let ids: Vec<String> = common().into_iter().map(|n| n.id).collect();
        for want in ["pool", "create_motion", "organize_audio_fx", "mount_audio_fx", "trim_clip", "transcript", "schedule_publication", "label_media", "folder"] {
            assert!(ids.iter().any(|i| i == want), "missing {want}");
        }
        for gone in ["thesis", "motion_source", "audio_fx", "schedule_post"] {
            assert!(!ids.iter().any(|i| i == gone), "{gone} should be gone");
        }
        let c = common();
        assert_eq!(c.iter().find(|n| n.id == "trim_clip").unwrap().extras["per"], "clip");
        assert_eq!(c.iter().find(|n| n.id == "folder").unwrap().extras["kind"], "folder");
        assert_eq!(display_name("organize_audio_fx"), "Organize Audio FX");
        assert_eq!(display_name("script_draft"), "Script Draft");
    }

    #[test]
    fn composed_graph_is_written_with_generic_flow() {
        let root = std::env::var("VENTOSA_GRAPH_TEST_DIR").map(std::path::PathBuf::from).unwrap_or_else(|_| std::env::temp_dir().join("ventosa-graph-test"));
        std::fs::create_dir_all(&root).unwrap();
        let c = common();
        let pick: Vec<NodeDef> = ["research", "review", "transcript", "research"].iter().map(|id| c.iter().find(|n| n.id == *id).unwrap().clone()).collect();
        write_graph(&root, "t", &pick).unwrap();
        let g: Value = serde_json::from_str(&std::fs::read_to_string(root.join("pipeline/skill-graph/steps.json")).unwrap()).unwrap();
        let ids: Vec<&str> = g["steps"].as_array().unwrap().iter().map(|s| s["id"].as_str().unwrap()).collect();
        assert_eq!(ids, ["research", "review", "transcript", "research_2", "done"], "duplicates get a suffix");
        assert!(root.join("pipeline/skill-graph/nodes/review.md").exists());
        let w: Value = serde_json::from_str(&std::fs::read_to_string(root.join("workspace.json")).unwrap()).unwrap();
        assert_eq!(w["flow"], "generic");
    }
}
