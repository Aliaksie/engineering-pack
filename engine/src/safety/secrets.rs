//! Files that commonly hold secrets, judged by name alone: `.env` files, private keys,
//! credential and token files, Terraform state and the credential directories of common
//! tools. Nothing is read from disk. Templates such as `.env.example` and public keys are not
//! secrets. A tool call or shell command that names such a file for reading is caught; a
//! read that does not name it (a script, `env`, a broad search) is outside what a name shows.

/// Endings that mark a template or a public half rather than a secret.
const NOT_SECRET: [&str; 7] = [
    ".pub",
    ".example",
    ".sample",
    ".template",
    ".tmpl",
    ".tpl",
    ".dist",
];
/// Leading words of a sample or fake key, as in `example-server-key.pem`.
const SAMPLE_PREFIXES: [&str; 4] = ["example", "sample", "dummy", "fake"];
/// Directories of test data and samples, where keys and credential files are fixtures.
const FIXTURES: [&str; 5] = [
    "testdata",
    "fixtures",
    "__fixtures__",
    "examples",
    "samples",
];
/// File names that hold credentials wherever they are.
const NAMES: [&str; 10] = [
    ".env",
    ".npmrc",
    ".pypirc",
    ".netrc",
    "_netrc",
    ".pgpass",
    ".git-credentials",
    ".dockercfg",
    ".htpasswd",
    "credentials",
];
/// Endings of private keys, keystores and Terraform state. A `.pem` file is as often a
/// public certificate, so only one named as a key counts (see [`private_pem`]).
const EXTENSIONS: [&str; 9] = [
    ".env",
    ".key",
    ".p12",
    ".pfx",
    ".jks",
    ".keystore",
    ".ppk",
    ".tfstate",
    ".tfstate.backup",
];
/// Default names of SSH private keys.
const SSH_KEYS: [&str; 4] = ["id_rsa", "id_dsa", "id_ecdsa", "id_ed25519"];
/// Data formats a `credentials` or `secrets` file is written in; source code is not one.
const DATA: [&str; 8] = [
    "json",
    "yml",
    "yaml",
    "toml",
    "ini",
    "xml",
    "properties",
    "env",
];
/// Files in `~/.ssh` that hold no key.
const SSH_PUBLIC: [&str; 5] = [
    "known_hosts",
    "known_hosts.old",
    "config",
    "authorized_keys",
    "rc",
];
/// A file in a tool's own directory that holds its tokens: (directory, file).
const TOOL_FILES: [(&str, &str); 7] = [
    ("gh", "hosts.yml"),
    (".kube", "config"),
    (".docker", "config.json"),
    (".azure", "accesstokens.json"),
    (".azure", "msal_token_cache.json"),
    ("gcloud", "credentials.db"),
    ("gcloud", "access_tokens.db"),
];

/// Commands that print, search, copy or send the files they name.
const READERS: [&str; 39] = [
    "cat",
    "tac",
    "less",
    "more",
    "head",
    "tail",
    "bat",
    "batcat",
    "nl",
    "od",
    "xxd",
    "hexdump",
    "strings",
    "base64",
    "base32",
    "grep",
    "egrep",
    "fgrep",
    "rg",
    "ag",
    "ack",
    "awk",
    "gawk",
    "sed",
    "cut",
    "sort",
    "uniq",
    "diff",
    "cmp",
    "comm",
    "jq",
    "yq",
    "openssl",
    "curl",
    "wget",
    "dd",
    "type",
    "get-content",
    "gc",
];
/// Commands whose last operand is where a copy goes, not a file that is read.
const COPIERS: [&str; 3] = ["cp", "scp", "rsync"];
/// Git commands that print a file's content, at a revision (`HEAD:.env`) or as a change.
const GIT_READERS: [&str; 6] = ["show", "diff", "blame", "grep", "cat-file", "log"];

/// Names that a search filter (`*.env`, `.env*`) may reach, and names any broad filter
/// reaches. A filter is about secrets only when it reaches one of the first and none of
/// the second: `*` and `*.json` are not.
const SECRET_SAMPLES: [&str; 13] = [
    ".env",
    ".env.local",
    "prod.env",
    "privkey.pem",
    "server.key",
    "id_rsa",
    "id_ed25519",
    ".npmrc",
    ".netrc",
    "terraform.tfstate",
    "credentials.json",
    "secrets.yaml",
    "store.p12",
];
const PLAIN_SAMPLES: [&str; 9] = [
    "main.rs",
    "index.ts",
    "readme.md",
    "notes.txt",
    "config.json",
    "data.yaml",
    "makefile",
    ".gitignore",
    "a",
];

/// A `.pem` file named as a private key: `privkey.pem`, `server-key.pem`, `tls_private.pem`;
/// not `cert.pem`, `cacert.pem`, `fullchain.pem` or `public_key.pem`.
fn private_pem(name: &str) -> bool {
    name.strip_suffix(".pem").is_some_and(|stem| {
        (stem.contains("key") || stem.contains("priv")) && !stem.contains("pub")
    })
}

/// A stem that names credentials or secrets as a whole word: `secrets`, `ci-secrets`,
/// `application_default_credentials`; not `nosecrets`.
fn credential_stem(stem: &str) -> bool {
    ["credentials", "secrets", "secret"].iter().any(|word| {
        stem.strip_suffix(word)
            .is_some_and(|before| before.is_empty() || before.ends_with(['_', '-', '.']))
    }) || stem.starts_with("client_secret")
}

/// Whether the file at `path` commonly holds secrets, by its name and the directories
/// above it. Either separator; case is ignored.
pub fn secret_file(path: &str) -> bool {
    let path = path.replace('\\', "/").to_ascii_lowercase();
    let segments: Vec<&str> = path
        .split('/')
        .filter(|segment| !segment.is_empty() && *segment != ".")
        .collect();
    let Some((&name, directories)) = segments.split_last() else {
        return false;
    };
    if NOT_SECRET.iter().any(|ending| name.ends_with(ending))
        || SAMPLE_PREFIXES
            .iter()
            .any(|prefix| name.starts_with(prefix))
        || directories
            .iter()
            .any(|directory| FIXTURES.contains(directory))
        || name == ".."
    {
        return false;
    }
    let parent = directories.last().copied().unwrap_or_default();
    if parent == ".ssh" {
        return !SSH_PUBLIC.contains(&name);
    }
    if directories.contains(&".gnupg")
        || TOOL_FILES
            .iter()
            .any(|(directory, file)| *directory == parent && *file == name)
    {
        return true;
    }
    if NAMES.contains(&name)
        || name.starts_with(".env.")
        || SSH_KEYS.iter().any(|key| name.starts_with(key))
        || private_pem(name)
        || EXTENSIONS
            .iter()
            .any(|ending| name.len() > ending.len() && name.ends_with(ending))
    {
        return true;
    }
    name.rsplit_once('.')
        .is_some_and(|(stem, extension)| DATA.contains(&extension) && credential_stem(stem))
}

/// `*`, `?` and one level of `{a,b}` alternatives against a lower-case name.
fn glob_match(pattern: &[char], name: &[char]) -> bool {
    match pattern.first() {
        None => name.is_empty(),
        Some('*') => (0..=name.len()).any(|skip| glob_match(&pattern[1..], &name[skip..])),
        Some('?') => !name.is_empty() && glob_match(&pattern[1..], &name[1..]),
        Some('{') => {
            let Some(close) = pattern.iter().position(|c| *c == '}') else {
                return name.first() == Some(&'{') && glob_match(&pattern[1..], &name[1..]);
            };
            pattern[1..close].split(|c| *c == ',').any(|choice| {
                let joined: Vec<char> = choice
                    .iter()
                    .chain(&pattern[close + 1..])
                    .copied()
                    .collect();
                glob_match(&joined, name)
            })
        }
        Some(c) => name.first() == Some(c) && glob_match(&pattern[1..], &name[1..]),
    }
}

/// Whether a search filter such as Claude `Grep`'s `glob` selects secret files: its last
/// segment reaches a secret name and no ordinary one. A filter without wildcards is a path.
pub fn secret_filter(filter: &str) -> bool {
    if !filter.contains(['*', '?', '{']) {
        return secret_file(filter);
    }
    let last = filter.replace('\\', "/").to_ascii_lowercase();
    let last: Vec<char> = last
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .chars()
        .collect();
    let matches = |name: &&str| glob_match(&last, &name.chars().collect::<Vec<_>>());
    SECRET_SAMPLES.iter().any(matches) && !PLAIN_SAMPLES.iter().any(matches)
}

fn basename(word: &str) -> &str {
    word.rsplit(['/', '\\']).next().unwrap_or(word)
}

/// The file one word of a reading command names: the value of `--option=value` or
/// `if=value`, a `@file` argument (`curl -d @.env`), or the word itself.
fn operand(word: &str) -> &str {
    let word = word.strip_prefix('@').unwrap_or(word);
    match word.split_once('=') {
        Some((option, value)) if option.starts_with('-') || option == "if" => value,
        _ => word,
    }
}

/// The secret file one shell command segment reads, if it names one: a reading command's
/// operand, the source of a copy, a file at a Git revision, or an input redirect (`< .env`)
/// on any command. Loading a file into the shell (`source .env`) shows the model nothing
/// and is not a read here.
pub fn shell_read(words: &[String]) -> Option<&str> {
    let redirected = words.iter().enumerate().find_map(|(index, word)| {
        let (_, after) = word.rsplit_once('<')?;
        if word.contains("<<") || word.contains("<(") {
            return None;
        }
        let file = if after.is_empty() {
            words.get(index + 1)?.as_str()
        } else {
            after
        };
        secret_file(file).then_some(file)
    });
    if redirected.is_some() {
        return redirected;
    }
    let (executable, args) = words.split_first()?;
    let executable = basename(executable).to_ascii_lowercase();
    let executable = executable.strip_suffix(".exe").unwrap_or(&executable);
    if READERS.contains(&executable) {
        return args
            .iter()
            .map(|word| operand(word))
            .find(|word| secret_file(word));
    }
    if COPIERS.contains(&executable) {
        let sources: Vec<usize> = (0..args.len())
            .filter(|index| !args[*index].starts_with('-'))
            .collect();
        let (_, sources) = sources.split_last()?;
        return sources
            .iter()
            .map(|index| operand(&args[*index]))
            .find(|word| secret_file(word));
    }
    if executable == "git" {
        let mut verb = 0;
        while args.get(verb)?.starts_with('-') {
            verb += 1 + usize::from(super::GIT_VALUE_OPTIONS.contains(&args[verb].as_str()));
        }
        if !GIT_READERS.contains(&args.get(verb)?.as_str()) {
            return None;
        }
        return args[verb + 1..].iter().find_map(|word| {
            let file = word.split_once(':').map_or(word.as_str(), |(_, path)| path);
            secret_file(file).then_some(file)
        });
    }
    None
}

#[cfg(test)]
mod tests;
