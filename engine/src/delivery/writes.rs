//! Files a shell command writes, read by name from its literal text: output redirections,
//! `tee`, in-place `sed` and `perl`, and the destination of `cp` and `mv`. A file written this
//! way gets the routing hint and standards check an edit gets, instead of waiting for the stop
//! review.
//!
//! Like the safety policy this reads words, not effects. Scripts, interpreters, generators,
//! formatters, patches and variables are outside it; the stop review still sees what they
//! wrote. Words are read without their quotes, so a quoted `>` reads as a redirection, and
//! `>|` is split at its `|` like any pipe.
use super::INTENT_FILES;
use crate::review::walk;

/// One written path as the command names it, relative to where the command runs. `sources`
/// are the files a copy or move takes: when `path` is a directory, each lands inside it.
#[derive(Debug, PartialEq)]
pub struct Written {
    pub path: String,
    pub sources: Vec<String>,
}

/// Words that change the directory later relative paths are read from.
const DIRECTORY_CHANGES: [&str; 3] = ["cd", "pushd", "popd"];

/// The files `command` writes, in order, without repeats and at most [`INTENT_FILES`]. After a
/// directory change, relative paths are no longer known and are left out.
pub fn shell_writes(command: &str) -> Vec<Written> {
    let mut written: Vec<Written> = Vec::new();
    let mut moved = false;
    walk(command, &mut |words| {
        let words = crate::safety::command(words);
        if words
            .first()
            .is_some_and(|w| DIRECTORY_CHANGES.contains(&basename(w)))
        {
            moved = true;
            return false;
        }
        let (words, mut found) = redirections(words);
        found.extend(writer(&words));
        for entry in found {
            let known = !moved || entry.path.starts_with('/') || drive(&entry.path);
            if known && literal(&entry.path) && !written.iter().any(|w| w.path == entry.path) {
                written.push(entry);
            }
        }
        written.len() >= INTENT_FILES
    });
    written.truncate(INTENT_FILES);
    written
}

fn basename(word: &str) -> &str {
    word.rsplit('/').next().unwrap_or(word)
}

fn drive(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'/' | b'\\')
}

/// A path named literally: no expansion, glob, home directory, device or standard stream.
fn literal(path: &str) -> bool {
    !path.is_empty()
        && path != "-"
        && !path.starts_with('~')
        && !path.starts_with("/dev/")
        && !path.contains(['$', '`', '*', '?', '[', '(', ')', '<', '>', '{', '}'])
}

/// The segment's words without its output redirections, and the files those write. `2>&1`
/// and `>&2` duplicate a stream; the segmenter splits them at `&`, so their remains name no
/// file. Input redirections are left in place: they read.
fn redirections(words: &[String]) -> (Vec<&str>, Vec<Written>) {
    let (mut rest, mut found) = (Vec::new(), Vec::new());
    let mut index = 0;
    while index < words.len() {
        let word = words[index].as_str();
        let operator = word.trim_start_matches(|c: char| c.is_ascii_digit());
        let Some(after) = operator.strip_prefix('>') else {
            rest.push(word);
            index += 1;
            continue;
        };
        let target = after.strip_prefix('>').unwrap_or(after);
        if target.is_empty() {
            if let Some(next) = words.get(index + 1) {
                found.push(file(next));
            }
            index += 2;
        } else {
            found.push(file(target));
            index += 1;
        }
    }
    (rest, found)
}

fn file(path: &str) -> Written {
    Written {
        path: path.to_string(),
        sources: Vec::new(),
    }
}

/// Files the command itself writes, by its name and options.
fn writer(words: &[&str]) -> Vec<Written> {
    let Some((executable, args)) = words.split_first() else {
        return Vec::new();
    };
    match basename(executable) {
        "tee" => operands(args, &[]).into_iter().map(file).collect(),
        "sed" => in_place(args, 'i', &["-e", "-f", "--expression", "--file", "-l"]),
        "perl" => perl(args),
        "cp" | "mv" => copy(args),
        _ => Vec::new(),
    }
}

/// Non-option words; `--` ends options, and each of `values` takes the next word.
fn operands<'w>(args: &[&'w str], values: &[&str]) -> Vec<&'w str> {
    let (mut operands, mut index, mut options) = (Vec::new(), 0, true);
    while index < args.len() {
        let word = args[index];
        if options && word == "--" {
            options = false;
        } else if options && word.starts_with('-') && word.len() > 1 {
            index += usize::from(values.contains(&word));
        } else {
            operands.push(word);
        }
        index += 1;
    }
    operands
}

/// A short option group holding `flag`, or the long `--in-place`.
fn edits_in_place(word: &str, flag: char) -> bool {
    word.starts_with("--in-place")
        || word
            .strip_prefix('-')
            .is_some_and(|group| !group.starts_with('-') && group.contains(flag))
}

/// `sed -i`: the files after the script, which is the first operand unless `-e` or `-f`
/// gives it. BSD's `-i ''` names an empty backup suffix, read as no word.
fn in_place(args: &[&str], flag: char, values: &[&str]) -> Vec<Written> {
    if !args.iter().any(|w| edits_in_place(w, flag)) {
        return Vec::new();
    }
    let args: Vec<&str> = args.iter().copied().filter(|w| !w.is_empty()).collect();
    let scripted = args
        .iter()
        .any(|w| values.contains(w) || w.starts_with("--expression=") || w.starts_with("--file="));
    let operands = operands(&args, values);
    let skip = usize::from(!scripted);
    operands.into_iter().skip(skip).map(file).collect()
}

/// `perl -i`: a group ending in `e` or `E` takes the script as the next word, as in
/// `-pi -e`; without one the first operand is the script file.
fn perl(args: &[&str]) -> Vec<Written> {
    if !args.iter().any(|w| edits_in_place(w, 'i')) {
        return Vec::new();
    }
    let (mut files, mut index, mut scripted) = (Vec::new(), 0, false);
    while index < args.len() {
        let word = args[index];
        if word == "--" {
            files.extend(args[index + 1..].iter().copied());
            break;
        }
        if word.starts_with('-') && word.len() > 1 {
            if word.ends_with(['e', 'E']) && !word.starts_with("--") {
                scripted = true;
                index += 1;
            }
        } else {
            files.push(word);
        }
        index += 1;
    }
    files
        .into_iter()
        .skip(usize::from(!scripted))
        .map(file)
        .collect()
}

/// `cp`/`mv`: the destination, or the target directory of `-t`, with the sources it takes.
fn copy(args: &[&str]) -> Vec<Written> {
    let mut directory = None;
    let mut index = 0;
    let mut plain = Vec::new();
    while index < args.len() {
        let word = args[index];
        if let Some(target) = word.strip_prefix("--target-directory=") {
            directory = Some(target);
        } else if word == "-t" || word == "--target-directory" {
            directory = args.get(index + 1).copied();
            index += 1;
        } else if word == "-S" || word == "--suffix" {
            index += 1;
        } else {
            plain.push(word);
        }
        index += 1;
    }
    let mut operands = operands(&plain, &[]);
    let path = match directory {
        Some(directory) => directory,
        None if operands.len() >= 2 => operands.pop().unwrap_or_default(),
        None => return Vec::new(),
    };
    vec![Written {
        path: path.to_string(),
        sources: operands.into_iter().map(str::to_string).collect(),
    }]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(command: &str) -> Vec<String> {
        shell_writes(command).into_iter().map(|w| w.path).collect()
    }

    #[test]
    fn redirections_name_the_file_they_write() {
        assert_eq!(paths("echo hi > notes.md"), ["notes.md"]);
        assert_eq!(paths("printf x >>log.txt"), ["log.txt"]);
        assert_eq!(paths("cmd 2>err.txt 1>out.txt"), ["err.txt", "out.txt"]);
        assert_eq!(paths("cargo test &> run.log"), ["run.log"]);
        assert_eq!(
            paths("cat > src/lib.rs <<'EOF'\nfn a() {}\nEOF"),
            ["src/lib.rs"]
        );
    }

    #[test]
    fn streams_devices_and_expansions_are_not_files() {
        for command in [
            "cargo test 2>&1 | tail -4",
            "echo x >&2",
            "make > /dev/null 2>&1",
            "echo x > $OUT",
            "echo x > ~/notes",
            "echo x > \"$(mktemp)\"",
            "cat < input.txt",
            "grep -n foo src/*.rs",
            "echo done",
        ] {
            assert!(paths(command).is_empty(), "{command}");
        }
    }

    #[test]
    fn tee_sed_and_perl_write_their_files() {
        assert_eq!(paths("ls | tee -a a.txt b.txt"), ["a.txt", "b.txt"]);
        assert_eq!(paths("sed -i 's/a/b/' x.rs y.rs"), ["x.rs", "y.rs"]);
        assert_eq!(paths("sed -i '' -e 's/a/b/' -e 's/c/d/' x.rs"), ["x.rs"]);
        assert_eq!(paths("sed -i.bak -f edit.sed x.rs"), ["x.rs"]);
        assert_eq!(paths("perl -pi -e 's/a/b/' x.rs"), ["x.rs"]);
        assert!(paths("sed 's/a/b/' x.rs").is_empty());
        assert!(paths("perl -ne 'print' x.rs").is_empty());
    }

    #[test]
    fn a_copy_or_move_names_its_destination_and_sources() {
        assert_eq!(
            shell_writes("cp -r a.rs b.rs dest/"),
            [Written {
                path: "dest/".into(),
                sources: vec!["a.rs".into(), "b.rs".into()]
            }]
        );
        assert_eq!(
            shell_writes("mv -t out a.rs"),
            [Written {
                path: "out".into(),
                sources: vec!["a.rs".into()]
            }]
        );
        assert!(paths("cp only-one").is_empty());
    }

    #[test]
    fn wrappers_shells_and_directory_changes_are_read() {
        assert_eq!(paths("A=1 timeout 5; bash -c 'echo x > a.txt'"), ["a.txt"]);
        assert_eq!(paths("echo x > a.txt && echo y > a.txt"), ["a.txt"]);
        assert_eq!(
            paths("cd sub && echo x > rel.txt; echo y > /abs/x.txt"),
            ["/abs/x.txt"]
        );
    }
}
