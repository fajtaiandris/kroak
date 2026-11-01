use std::{
    fs,
    os::unix::fs::DirBuilderExt,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{anyhow, Context, Result};
use serde::Deserialize;

use crate::{Look, Reasoning};

const SCHEMA: &str = include_str!("../schema.json");
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Result<Self> {
        let base = std::env::temp_dir();
        for _ in 0..16 {
            let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
            let sequence = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
            let path = base.join(format!("kroak-{}-{nonce}-{sequence}", std::process::id()));
            let mut builder = fs::DirBuilder::new();
            builder.mode(0o700);
            match builder.create(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error.into()),
            }
        }
        Err(anyhow!("could not create a unique temporary directory"))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[derive(Deserialize)]
struct Answer {
    short_answer: String,
}

fn prompt(question: &str, previous: Option<(&str, &str)>, look: Look, launch_dir: &Path) -> String {
    let context = match look {
        Look::Dont => "Do not inspect any files, directories, tools, or web pages. Answer from the question alone.".to_string(),
        Look::Look => format!(
            "Your working directory is {}. You may read files and use read-only Git commands here to answer. Do not write or change any files. Do not inspect unrelated directories or use the web.",
            launch_dir.display()
        ),
    };
    let prior = previous.map_or_else(String::new, |(question, answer)| {
        format!("Previous question: {question}\nPrevious answer: {answer}\n\n")
    });
    format!(
        "You are a short-answer helper. Answer the user's exact question directly and briefly. No preamble or follow-up. If code is useful, put it in a fenced code block with a language tag. If you are unsure, say so. {context}\n\n{prior}Question: {question}"
    )
}

fn command(program: &Path, working_root: &Path, schema: &Path, reasoning: Reasoning) -> Command {
    let mut cmd = Command::new(program);
    cmd.arg("exec")
        .arg("--cd")
        .arg(working_root)
        .arg("--ephemeral")
        .arg("--skip-git-repo-check")
        .arg("--ignore-user-config")
        .arg("--model")
        .arg("gpt-6-luna")
        .arg("-c")
        .arg(format!("model_reasoning_effort=\"{}\"", reasoning.effort()))
        .arg("-c")
        .arg("approval_policy=\"never\"")
        .arg("--sandbox")
        .arg("read-only")
        .arg("--output-schema")
        .arg(schema)
        .current_dir(working_root)
        .env("PWD", working_root);
    cmd
}

pub fn ask(
    question: &str,
    previous: Option<(&str, &str)>,
    reasoning: Reasoning,
    look: Look,
    launch_dir: &Path,
) -> Result<String> {
    ask_with_program(
        Path::new("codex"),
        question,
        previous,
        reasoning,
        look,
        launch_dir,
    )
}

fn ask_with_program(
    program: &Path,
    question: &str,
    previous: Option<(&str, &str)>,
    reasoning: Reasoning,
    look: Look,
    launch_dir: &Path,
) -> Result<String> {
    // The schema lives outside the empty workspace. Don't look uses that workspace.
    let scratch = Scratch::new().context("could not create a temporary directory")?;
    let workspace = scratch.0.join("workspace");
    fs::create_dir(&workspace).context("could not create the empty workspace")?;
    let schema = scratch.0.join("schema.json");
    fs::write(&schema, SCHEMA).context("could not prepare the answer schema")?;

    let working_root = match look {
        Look::Dont => workspace.as_path(),
        Look::Look => launch_dir,
    };
    let mut cmd = command(program, working_root, &schema, reasoning);
    let output = cmd
        .arg(prompt(question, previous, look, launch_dir))
        .output()
        .context("could not start codex; install the Codex CLI and sign in first")?;
    if !output.status.success() {
        let details = String::from_utf8_lossy(&output.stderr);
        let last_line = details.lines().rev().find(|line| !line.trim().is_empty());
        return Err(anyhow!(
            "codex failed: {}",
            last_line.unwrap_or("no error details were returned")
        ));
    }
    let answer: Answer =
        serde_json::from_slice(&output.stdout).context("codex returned an invalid answer")?;
    let answer = answer.short_answer.trim();
    if answer.is_empty() {
        return Err(anyhow!("codex returned an empty answer"));
    }
    Ok(answer.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_keeps_workspace_read_only_and_ephemeral() {
        let cmd = command(
            Path::new("codex"),
            Path::new("/tmp/empty"),
            Path::new("/tmp/schema"),
            Reasoning::Ooh,
        );
        let args: Vec<_> = cmd
            .get_args()
            .map(|x| x.to_string_lossy().into_owned())
            .collect();
        assert!(args.windows(2).any(|pair| pair == ["--cd", "/tmp/empty"]));
        assert!(args
            .windows(2)
            .any(|pair| pair == ["--sandbox", "read-only"]));
        assert!(args.contains(&"--ephemeral".to_string()));
        assert!(args.contains(&"model_reasoning_effort=\"high\"".to_string()));
        assert_eq!(cmd.get_current_dir(), Some(Path::new("/tmp/empty")));
        assert!(cmd.get_envs().any(|(key, value)| {
            key == "PWD" && value == Some(std::ffi::OsStr::new("/tmp/empty"))
        }));
    }

    #[test]
    fn workspace_starts_empty_and_is_removed() {
        let scratch = Scratch::new().unwrap();
        let path = scratch.0.clone();
        let workspace = path.join("workspace");
        fs::create_dir(&workspace).unwrap();
        fs::write(path.join("schema.json"), SCHEMA).unwrap();
        assert_eq!(fs::read_dir(&workspace).unwrap().count(), 0);
        drop(scratch);
        assert!(!path.exists());
    }

    #[test]
    fn runs_mock_codex_in_empty_workspace_and_parses_answer() {
        use std::os::unix::fs::PermissionsExt;

        let mock = Scratch::new().unwrap();
        let program = mock.0.join("codex");
        fs::write(
            &program,
            "#!/bin/sh\n[ \"$PWD\" = \"$(pwd)\" ] || exit 3\n[ -z \"$(ls -A)\" ] || exit 4\nprintf '{\"short_answer\":\"Seven.\"}\\n'\n",
        )
        .unwrap();
        fs::set_permissions(&program, fs::Permissions::from_mode(0o700)).unwrap();
        let answer = ask_with_program(
            &program,
            "4+3",
            None,
            Reasoning::Duh,
            Look::Dont,
            Path::new("/tmp/project"),
        )
        .unwrap();
        assert_eq!(answer, "Seven.");
    }

    #[test]
    fn look_runs_in_launch_repo_and_can_read_unstaged_changes() {
        use std::os::unix::fs::PermissionsExt;

        let repo = Scratch::new().unwrap();
        assert!(Command::new("git")
            .args(["init", "-q"])
            .current_dir(&repo.0)
            .status()
            .unwrap()
            .success());
        fs::write(repo.0.join("tracked.txt"), "before\n").unwrap();
        assert!(Command::new("git")
            .args(["add", "tracked.txt"])
            .current_dir(&repo.0)
            .status()
            .unwrap()
            .success());
        fs::write(repo.0.join("tracked.txt"), "after\n").unwrap();

        let mock = Scratch::new().unwrap();
        let program = mock.0.join("codex");
        fs::write(
            &program,
            "#!/bin/sh\n[ \"$PWD\" = \"$(pwd)\" ] || exit 3\n[ -d .git ] || exit 4\n[ \"$(git diff --name-only)\" = tracked.txt ] || exit 5\nprintf '{\"short_answer\":\"tracked.txt is modified.\"}\\n'\n",
        )
        .unwrap();
        fs::set_permissions(&program, fs::Permissions::from_mode(0o700)).unwrap();
        let answer = ask_with_program(
            &program,
            "What are my unstaged changes?",
            None,
            Reasoning::Duh,
            Look::Look,
            &repo.0,
        )
        .unwrap();
        assert_eq!(answer, "tracked.txt is modified.");
    }

    #[test]
    fn only_look_mentions_launch_directory() {
        let path = Path::new("/private/example-project");
        assert!(!prompt("hi", None, Look::Dont, path).contains("example-project"));
        assert!(prompt("hi", None, Look::Look, path).contains("example-project"));
        assert!(prompt("What changed?", None, Look::Look, path).contains("read-only Git commands"));
    }

    #[test]
    fn followup_prompt_includes_only_the_previous_exchange() {
        let path = Path::new("/tmp/project");
        let fresh = prompt("How?", None, Look::Dont, path);
        assert!(!fresh.contains("Previous question:"));
        let followup = prompt("How?", Some(("What?", "A frog.")), Look::Dont, path);
        assert!(followup
            .contains("Previous question: What?\nPrevious answer: A frog.\n\nQuestion: How?"));
    }
}
