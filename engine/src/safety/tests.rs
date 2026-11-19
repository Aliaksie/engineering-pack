use super::*;

const ROOT: &str = "/work/shop";

fn at<'a>(roots: &'a [String], cwd: &'a str, temp: &'a [String]) -> Place<'a> {
    Place { roots, cwd, temp }
}

fn judged(settings: &Value, place: &Place, command: &str) -> Option<String> {
    let decision = decision(settings, place, command);
    match decision["decision"].as_str() {
        Some("allow") => None,
        Some("deny") => Some(decision["rule"].as_str().expect("rule").to_string()),
        other => panic!("decision {other:?}"),
    }
}

fn on() -> Value {
    json!({"safety": {"enabled": true}})
}

fn rule(settings: &Value, cwd: &str, command: &str) -> Option<String> {
    judged(settings, &at(&[ROOT.into()], cwd, &[]), command)
}

fn refused(command: &str) -> Option<String> {
    rule(&on(), ROOT, command)
}

#[test]
fn the_policy_is_off_until_the_project_turns_it_on() {
    for settings in [
        json!({}),
        json!({"safety": {"enabled": false}}),
        json!({"safety": {}}),
    ] {
        assert_eq!(rule(&settings, ROOT, "sudo rm -rf /"), None);
    }
}

#[test]
fn a_recursive_delete_is_judged_by_where_its_target_lies() {
    for command in [
        "rm -rf /",
        "rm -rf ~",
        "rm -rf ~/projects",
        "rm -r /etc/nginx",
        "rm -rf ..",
        "rm -rf ../other",
        "rm -rf src/../../x",
        "rm -rf \"$BUILD_DIR\"",
        "rm -rf $HOME/cache",
        "rm --recursive /var/tmp/x",
        "/bin/rm -Rf /opt/app",
        "rm -rf build /tmp/elsewhere",
        "rm -rf -- /srv",
    ] {
        assert_eq!(
            refused(command).as_deref(),
            Some("delete-outside-project"),
            "{command}"
        );
    }
    for command in [
        "rm -rf .",
        "rm -rf ./",
        "rm -rf *",
        "rm -rf .git",
        "rm -rf /work/shop",
        "rm -fr ./*",
    ] {
        assert_eq!(
            refused(command).as_deref(),
            Some("delete-project-root"),
            "{command}"
        );
    }
    for command in [
        "rm -rf build",
        "rm -rf ./target/debug",
        "rm -rf /work/shop/dist",
        "rm -rf node_modules dist",
        "rm /etc/hosts",
        "rm -f notes.txt",
        "rm -rf src/*",
        "rm -rf packages/a/.git",
    ] {
        assert_eq!(refused(command), None, "{command}");
    }
}

#[test]
fn the_working_directory_decides_what_a_relative_target_means() {
    let settings = on();
    assert_eq!(
        rule(&settings, "/work/shop/api", "rm -rf ..").as_deref(),
        Some("delete-project-root")
    );
    assert_eq!(rule(&settings, "/work/shop/api", "rm -rf *"), None);
    assert_eq!(
        rule(&settings, "/work/shop/api", "rm -rf ../.."),
        Some("delete-outside-project".into())
    );
    assert_eq!(rule(&settings, "/work", "rm -rf shop/build"), None);
}

#[test]
fn a_forced_push_is_refused_and_a_leased_one_is_not() {
    for command in [
        "git push --force",
        "git push -f origin main",
        "git push origin +main",
        "git -C sub push --force origin main",
        "git push --mirror",
        "git push -uf origin x",
    ] {
        assert_eq!(refused(command).as_deref(), Some("force-push"), "{command}");
    }
    for command in [
        "git push",
        "git push origin main",
        "git push --force-with-lease",
        "git push -u origin feat",
    ] {
        assert_eq!(refused(command), None, "{command}");
    }
}

#[test]
fn git_commands_that_discard_work_are_refused() {
    for command in [
        "git reset --hard",
        "git reset --hard HEAD~1",
        "git checkout -- .",
        "git checkout .",
        "git checkout -f main",
        "git restore .",
        "git restore --staged --worktree .",
        "git clean -fd",
        "git clean -xfd",
        "git clean --force",
        "git stash drop",
        "git stash clear",
    ] {
        assert_eq!(
            refused(command).as_deref(),
            Some("discard-changes"),
            "{command}"
        );
    }
    for command in [
        "git reset --soft HEAD~1",
        "git reset HEAD file.rs",
        "git checkout main",
        "git checkout -b feat",
        "git checkout -- src/a.rs",
        "git restore --staged .",
        "git restore src/a.rs",
        "git clean -n",
        "git clean -nfd",
        "git stash",
        "git stash pop",
        "git status",
    ] {
        assert_eq!(refused(command), None, "{command}");
    }
}

#[test]
fn privileged_and_system_commands_are_refused() {
    for command in [
        "sudo apt install jq",
        "reboot",
        "shutdown -h now",
        "systemctl stop nginx",
        "systemctl --user disable foo.service",
        "service nginx stop",
        "mount /dev/sdb1 /mnt",
        "umount /mnt",
        "mkfs.ext4 /dev/sdb1",
        "dd if=image.iso of=/dev/sda bs=4M",
        "launchctl bootout system/com.x",
    ] {
        assert_eq!(
            refused(command).as_deref(),
            Some("system-administration"),
            "{command}"
        );
    }
    for command in [
        "systemctl status nginx",
        "mount",
        "dd if=a of=b",
        "echo sudo",
        "cargo test",
        "npm run mount",
    ] {
        assert_eq!(refused(command), None, "{command}");
    }
}

#[test]
fn compound_wrapped_and_nested_commands_are_read() {
    for (command, expected) in [
        ("cd /tmp && rm -rf /var/x", "delete-outside-project"),
        ("npm test; git push --force", "force-push"),
        ("FOO=1 BAR=2 git reset --hard", "discard-changes"),
        ("env -i git clean -fd", "discard-changes"),
        ("nohup sudo reboot", "system-administration"),
        ("bash -c 'git push -f'", "force-push"),
        ("sh -lc \"rm -rf ~\"", "delete-outside-project"),
        ("echo $(git stash clear)", "discard-changes"),
        ("echo `rm -rf /x`", "delete-outside-project"),
        (
            "true | xargs echo; time git reset --hard",
            "discard-changes",
        ),
    ] {
        assert_eq!(refused(command).as_deref(), Some(expected), "{command}");
    }
    for command in [
        "echo 'git push --force'",
        "git commit -m 'rm -rf / is bad'",
        "grep -r \"reset --hard\" docs",
    ] {
        assert_eq!(refused(command), None, "{command}");
    }
    let nested = (0..20).fold("true".to_string(), |inner, _| format!("echo $({inner})"));
    assert!(refused(&nested).is_some(), "too deep to read is refused");
}

#[test]
fn a_disabled_rule_no_longer_refuses() {
    let settings = json!({"safety": {"enabled": true, "disables": ["discard-changes"]}});
    assert_eq!(rule(&settings, ROOT, "git reset --hard"), None);
    assert_eq!(
        rule(&settings, ROOT, "git push -f").as_deref(),
        Some("force-push")
    );
}

#[test]
fn a_project_rule_refuses_commands_by_their_leading_words() {
    let settings = json!({"safety": {"enabled": true, "deny": [
        {"id": "infra-destroy", "command": ["terraform", "destroy"], "reason": "tearing down shared infrastructure"},
        {"id": "no-publish", "command": ["npm", "publish"]}
    ]}});
    assert_eq!(
        rule(&settings, ROOT, "terraform destroy -auto-approve").as_deref(),
        Some("infra-destroy")
    );
    assert_eq!(
        rule(&settings, ROOT, "cd infra && /usr/bin/terraform destroy").as_deref(),
        Some("infra-destroy")
    );
    assert_eq!(rule(&settings, ROOT, "terraform plan -destroy"), None);
    assert_eq!(
        rule(&settings, ROOT, "npm publish").as_deref(),
        Some("no-publish")
    );
    assert_eq!(rule(&settings, ROOT, "npm"), None);
    let denied = decision(
        &settings,
        &at(&[ROOT.into()], ROOT, &[]),
        "terraform destroy",
    );
    let reason = denied["reason"].as_str().expect("reason");
    assert!(
        reason.contains("rule `infra-destroy`: tearing down shared infrastructure"),
        "{reason}"
    );
}

#[test]
fn the_reason_names_the_rule_and_never_how_to_turn_it_off() {
    let denied = decision(&on(), &at(&[ROOT.into()], ROOT, &[]), "git push --force");
    let reason = denied["reason"].as_str().expect("reason");
    assert!(reason.contains("rule `force-push`"), "{reason}");
    for word in ["disables", "enabled", "project.json", "safety."] {
        assert!(!reason.contains(word), "{reason}");
    }
}

#[test]
fn configuration_is_validated() {
    for valid in [
        json!({}),
        json!({"enabled": true, "disables": ["force-push"]}),
        json!({"deny": [{"id": "x", "command": ["a", "b"], "reason": "why"}]}),
    ] {
        validate(&valid).unwrap_or_else(|error| panic!("{valid}: {error}"));
    }
    for invalid in [
        json!({"enabled": "yes"}),
        json!({"on": true}),
        json!({"disables": ["no-such-rule"]}),
        json!({"deny": [{"id": "force-push", "command": ["a"]}]}),
        json!({"deny": [{"id": "x", "command": []}]}),
        json!({"deny": [{"id": "x", "command": ["a b"]}]}),
        json!({"deny": [{"id": "x", "command": ["a"]}, {"id": "x", "command": ["b"]}]}),
        json!({"deny": [{"id": "X", "command": ["a"]}]}),
        json!({"deny": [{"id": "x", "command": ["a"], "allow": true}]}),
    ] {
        assert!(validate(&invalid).is_err(), "{invalid}");
    }
}

#[test]
fn the_listing_shows_rules_in_force_with_their_source() {
    let settings = json!({"safety": {"enabled": true, "disables": ["system-administration"],
        "deny": [{"id": "no-publish", "command": ["npm", "publish"]}]}});
    let listing = listing(&settings);
    assert_eq!(listing["enabled"], true);
    assert_eq!(listing["disabled"], json!(["system-administration"]));
    let ids: Vec<(&str, &str)> = listing["rules"]
        .as_array()
        .expect("rules")
        .iter()
        .map(|rule| {
            (
                rule["id"].as_str().unwrap(),
                rule["source"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        ids,
        [
            ("delete-outside-project", "craft"),
            ("delete-project-root", "craft"),
            ("force-push", "craft"),
            ("delete-remote-branch", "craft"),
            ("discard-changes", "craft"),
            ("read-secret-file", "craft"),
            ("no-publish", "project"),
        ]
    );
    let off = super::listing(&json!({}));
    assert_eq!(
        (off["enabled"].clone(), off["origin"].clone()),
        (json!(false), json!("bundled"))
    );
}

#[test]
fn the_text_of_a_here_document_is_data_unless_a_shell_runs_it() {
    for command in [
        "cat > notes.md <<'EOF'\nThe session ran rm -rf \"$SCRATCH\" and sudo reboot.\nEOF",
        "cat <<EOF > notes.md\ngit push --force is refused; don't run it\nEOF\necho done",
        "cat <<-EOF\n\trm -rf ~\n\tEOF",
        "cat <<\\EOF\nrm -rf ~\nEOF",
        "cat <<A <<'B'\nrm -rf ~\nA\nsudo reboot\nB",
        "git commit -m \"$(cat <<'EOF'\nfix: rm -rf / is refused\nEOF\n)\"",
        "tee a.txt <<< 'sudo reboot'",
    ] {
        assert_eq!(refused(command), None, "{command}");
    }
    for (command, rule) in [
        // What follows the document is a command again.
        ("cat <<EOF\ntext\nEOF\nrm -rf ~", "delete-outside-project"),
        (
            "cat <<'EOF' && sudo reboot\ntext\nEOF",
            "system-administration",
        ),
        // A shell reads its document as a script.
        ("bash <<'EOF'\nrm -rf ~\nEOF", "delete-outside-project"),
        ("cat <<EOF | /bin/sh\ngit push -f\nEOF", "force-push"),
        (
            "env X=1 zsh -s <<EOF\nsudo reboot\nEOF",
            "system-administration",
        ),
        // An unquoted delimiter leaves substitutions in the text active.
        ("cat <<EOF\nit's $(rm -rf ~)\nEOF", "delete-outside-project"),
        ("cat <<EOF\n`git reset --hard`\nEOF", "discard-changes"),
        // Without a closing line `<<` hides nothing that follows.
        ("echo $((1<<2))\nrm -rf ~", "delete-outside-project"),
        ("cat <<EOF\nrm -rf ~", "delete-outside-project"),
    ] {
        assert_eq!(refused(command).as_deref(), Some(rule), "{command}");
    }
    // A quoted delimiter makes the text literal.
    assert_eq!(refused("cat <<'EOF'\n$(rm -rf ~)\nEOF"), None);
}

#[test]
fn a_recursive_delete_strictly_below_a_temporary_directory_is_allowed() {
    let roots = [ROOT.to_string()];
    let temp = ["/tmp".to_string(), "/private/var/folders/x1/T".to_string()];
    let place = at(&roots, ROOT, &temp);
    for command in [
        "rm -rf /tmp/claude-501/session/scratchpad/out",
        "rm -rf /tmp/build-cache",
        "rm -rf /private/var/folders/x1/T/craft-probe",
    ] {
        assert_eq!(judged(&on(), &place, command), None, "{command}");
    }
    for command in [
        "rm -rf /tmp",
        "rm -rf /tmp/",
        "rm -rf /tmp/*",
        "rm -rf /tmp/../etc",
        "rm -rf \"$TMPDIR/x\"",
        "rm -rf /var/tmp/x",
        "rm -rf /private/var/folders/x1",
    ] {
        assert_eq!(
            judged(&on(), &place, command).as_deref(),
            Some("delete-outside-project"),
            "{command}"
        );
    }
}

#[test]
fn a_delete_of_a_variable_target_is_refused_with_the_literal_paths_that_pass() {
    let roots = [ROOT.to_string()];
    let temp = ["/private/tmp".to_string()];
    let place = at(&roots, ROOT, &temp);
    let scratch = "/private/tmp/claude-501/session/scratchpad";
    // The host's scratch directory written out passes.
    assert_eq!(
        judged(&on(), &place, &format!("rm -rf {scratch}/clone")),
        None
    );
    for (command, target) in [
        ("rm -rf \"$SCRATCH/clone\"", "$SCRATCH/clone"),
        ("rm -rf ${SCRATCH}/clone build", "${SCRATCH}/clone"),
        ("rm -rf \"$(mktemp -d)\"", "$(mktemp -d)"),
        ("find \"$SCRATCH\" -name '*.o' -delete", "$SCRATCH"),
        (
            &format!("SCRATCH={scratch}; rm -rf \"$SCRATCH/clone\""),
            "$SCRATCH/clone",
        ),
    ] {
        let decision = decision(&on(), &place, command);
        assert_eq!(decision["rule"], "delete-outside-project", "{command}");
        let reason = decision["reason"].as_str().expect("reason");
        assert!(
            reason.contains(&format!("`{target}`")),
            "{command}: {reason}"
        );
        assert!(reason.contains("does not expand"), "{reason}");
        assert!(reason.contains("temporary directory"), "{reason}");
    }
    // A target outside the project keeps the rule's own wording, before any variable.
    let outside = decision(&on(), &place, "rm -rf \"$SCRATCH/clone\" /srv");
    assert_eq!(outside["rule"], "delete-outside-project");
    assert!(
        !outside["reason"]
            .as_str()
            .unwrap()
            .contains("does not expand")
    );
    let home = decision(&on(), &place, "rm -rf ~/cache");
    assert!(!home["reason"].as_str().unwrap().contains("does not expand"));
    // Turning the rule off covers its variable case too.
    let off = json!({"safety": {"enabled": true, "disables": ["delete-outside-project"]}});
    assert_eq!(judged(&off, &place, "rm -rf \"$SCRATCH/clone\""), None);
}

#[test]
fn the_project_root_is_recognized_under_each_of_its_spellings() {
    let roots = ["/tmp/shop".to_string(), "/private/tmp/shop".to_string()];
    let place = at(&roots, "/private/tmp/shop", &[]);
    assert_eq!(judged(&on(), &place, "rm -rf /tmp/shop/build"), None);
    assert_eq!(judged(&on(), &place, "rm -rf build"), None);
    assert_eq!(
        judged(&on(), &place, "rm -rf /tmp/shop").as_deref(),
        Some("delete-project-root")
    );
}

#[test]
fn windows_drive_and_network_paths_are_judged_as_absolute() {
    let roots = ["C:/work/shop".to_string()];
    let temp = ["C:/Users/me/AppData/Local/Temp".to_string()];
    let place = at(&roots, "C:/work/shop", &temp);
    for command in [
        "rm -rf D:/data",
        "rm -rf D:\\data",
        "rm -rf C:\\Users\\me",
        "rm -rf \\\\server\\share\\x",
        "rm -rf D:",
        "rm -rf D:data",
    ] {
        assert_eq!(
            judged(&on(), &place, command).as_deref(),
            Some("delete-outside-project"),
            "{command}"
        );
    }
    for command in [
        "rm -rf C:/work/shop/build",
        "rm -rf c:\\work\\shop\\dist",
        "rm -rf build",
        "rm -rf C:\\Users\\me\\AppData\\Local\\Temp\\craft",
    ] {
        assert_eq!(judged(&on(), &place, command), None, "{command}");
    }
    assert_eq!(
        judged(&on(), &place, "rm -rf C:\\work\\shop").as_deref(),
        Some("delete-project-root")
    );
}

#[test]
fn deletes_through_xargs_and_find_are_read() {
    for (command, expected) in [
        (
            "find build -name '*.o' | xargs rm -rf",
            Some("delete-outside-project"),
        ),
        (
            "git ls-files -z | xargs -0 -n 1 rm -r",
            Some("delete-outside-project"),
        ),
        (
            "xargs -I {} rm -rf {} < list.txt",
            Some("delete-outside-project"),
        ),
        ("ls | xargs rm -f", None),
        ("ls | xargs git push --force", Some("force-push")),
        (
            "find / -name '*.log' -delete",
            Some("delete-outside-project"),
        ),
        ("find ~/cache -delete", Some("delete-outside-project")),
        ("find . -delete", Some("delete-project-root")),
        ("find -mindepth 1 -delete", Some("delete-project-root")),
        (
            "find . -type d -exec rm -rf {} +",
            Some("delete-project-root"),
        ),
        (
            "find /srv -name x -exec rm -rf {} \\;",
            Some("delete-outside-project"),
        ),
        ("find . -name '*.pyc' -delete", None),
        ("find build -delete", None),
        ("find . -name node_modules -type d -exec rm -rf {} +", None),
        ("find / -name x -exec rm {} \\;", None),
        ("find / -name '*.log'", None),
    ] {
        assert_eq!(refused(command).as_deref(), expected, "{command}");
    }
}

#[test]
fn a_push_that_deletes_a_remote_branch_is_refused() {
    for command in [
        "git push origin --delete feature",
        "git push -d origin feature",
        "git push origin :feature",
        "git push --prune origin",
        "git push origin :refs/tags/v1",
    ] {
        assert_eq!(
            refused(command).as_deref(),
            Some("delete-remote-branch"),
            "{command}"
        );
    }
    for command in [
        "git push origin feature",
        "git push origin HEAD:feature",
        "git push -u origin x",
    ] {
        assert_eq!(refused(command), None, "{command}");
    }
    let settings = json!({"safety": {"enabled": true, "disables": ["delete-remote-branch"]}});
    assert_eq!(rule(&settings, ROOT, "git push origin :feature"), None);
}

#[test]
fn a_shell_read_of_a_secret_file_is_refused_and_may_go_to_the_user() {
    let decision = decision(
        &on(),
        &at(&[ROOT.into()], ROOT, &[]),
        "cd app && cat .env | head",
    );
    assert_eq!(decision["decision"], "deny");
    assert_eq!(decision["rule"], "read-secret-file");
    assert_eq!(decision["file"], ".env");
    assert!(
        decision["ask"]
            .as_str()
            .is_some_and(|ask| ask.contains("`.env`"))
    );
    // A destructive command is judged first and has no user hand-off.
    let destructive = decision_for("cat .env; rm -rf /");
    assert_eq!(destructive["rule"], "read-secret-file");
    assert_eq!(
        refused("rm -rf / ; cat .env").as_deref(),
        Some("delete-outside-project")
    );
    assert!(decision_for("rm -rf /")["ask"].is_null());
    assert_eq!(
        refused("bash -c 'cat ~/.aws/credentials'").as_deref(),
        Some("read-secret-file")
    );
    assert_eq!(
        refused("echo $(cat .env)").as_deref(),
        Some("read-secret-file")
    );
    assert_eq!(refused("cat .env.example"), None);
}

fn decision_for(command: &str) -> Value {
    decision(&on(), &at(&[ROOT.into()], ROOT, &[]), command)
}

#[test]
fn the_secret_file_rule_can_be_disabled_like_any_built_in_rule() {
    let off = json!({"safety": {"enabled": true, "disables": ["read-secret-file"]}});
    assert_eq!(rule(&off, ROOT, "cat .env"), None);
    assert_eq!(read_decision(&off, ".env")["decision"], "allow");
    assert_eq!(read_decision(&json!({}), ".env")["decision"], "allow");
    let on = read_decision(&on(), "config/.env");
    assert_eq!(
        (on["decision"].as_str(), on["rule"].as_str()),
        (Some("deny"), Some("read-secret-file"))
    );
    assert!(validate(&json!({"disables": ["read-secret-file"]})).is_ok());
}

#[test]
fn a_tool_call_names_a_secret_by_a_file_or_a_search_filter() {
    assert_eq!(
        named_secret(&["src/main.rs", "app/.env"], &[]),
        Some("app/.env")
    );
    assert_eq!(named_secret(&["src"], &["*.pem"]), Some("*.pem"));
    assert_eq!(named_secret(&["src", ".env.example"], &["*.rs", "*"]), None);
}

fn vault() -> Value {
    json!({"safety": {"enabled": true,
        "secretFiles": [{"id": "vault-files", "pattern": "*.vault"}]}})
}

#[test]
fn a_declared_secret_file_is_refused_like_a_built_in_one() {
    let settings = vault();
    let roots = [ROOT.to_string()];
    let place = at(&roots, ROOT, &[]);
    assert_eq!(
        judged(&settings, &place, "cat conf/prod.vault").as_deref(),
        Some("read-secret-file")
    );
    assert_eq!(judged(&on(), &place, "cat conf/prod.vault"), None);
    assert_eq!(
        declared_secret(&settings, &["src/main.rs", "conf/prod.vault"], &[]),
        Some("conf/prod.vault")
    );
    assert_eq!(
        declared_secret(&settings, &["src"], &["*.vault"]),
        Some("*.vault")
    );
    assert_eq!(
        declared_secret(&settings, &["app/.env"], &[]),
        Some("app/.env")
    );
    assert_eq!(named_secret(&["conf/prod.vault"], &[]), None);
    let off = json!({"safety": {"enabled": true, "disables": ["read-secret-file"],
        "secretFiles": [{"id": "vault-files", "pattern": "*.vault"}]}});
    assert_eq!(judged(&off, &place, "cat conf/prod.vault"), None);
    assert_eq!(read_decision(&off, "conf/prod.vault")["decision"], "allow");
}

#[test]
fn declared_secret_files_are_validated() {
    for valid in [
        json!({"secretFiles": []}),
        json!({"secretFiles": [{"id": "vault", "pattern": "*.vault"}, {"id": "old", "disabled": true}]}),
    ] {
        validate(&valid).unwrap_or_else(|error| panic!("{valid}: {error}"));
    }
    let many: Vec<Value> = (0..33)
        .map(|n| json!({"id": format!("p{n}"), "pattern": format!("{n}.vault")}))
        .collect();
    for invalid in [
        json!({"secretFiles": {"id": "x"}}),
        json!({"secretFiles": many}),
        json!({"secretFiles": [{"id": "X", "pattern": "*.vault"}]}),
        json!({"secretFiles": [{"id": "x"}]}),
        json!({"secretFiles": [{"id": "x", "pattern": 1}]}),
        json!({"secretFiles": [{"id": "x", "pattern": "*"}]}),
        json!({"secretFiles": [{"id": "x", "pattern": "a.vault"}, {"id": "x", "pattern": "b.vault"}]}),
        json!({"secretFiles": [{"id": "x", "pattern": "a.vault", "disabled": true}]}),
        json!({"secretFiles": [{"id": "x", "pattern": "a.vault", "reason": "why"}]}),
    ] {
        assert!(validate(&invalid).is_err(), "{invalid}");
    }
}

#[test]
fn the_listing_names_the_layer_of_each_declared_secret_file() {
    let resolved = json!({
        "settings": {"safety": {"enabled": true, "secretFiles": [
            {"id": "vault-files", "pattern": "*.vault"},
            {"id": "acme-token", "pattern": ".acme/token"}]}},
        "origins": {},
        "entries": {"safety.secretFiles": {"vault-files": "organization", "acme-token": "user"}},
    });
    assert_eq!(
        layered_listing(&resolved)["secretFiles"],
        json!([
            {"id": "vault-files", "pattern": "*.vault", "source": "organization"},
            {"id": "acme-token", "pattern": ".acme/token", "source": "user"},
        ])
    );
    assert_eq!(listing(&on())["secretFiles"], json!([]));
}
