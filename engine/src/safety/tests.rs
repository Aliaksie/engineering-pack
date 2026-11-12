use super::*;

const ROOT: &str = "/work/shop";

fn on() -> Value {
    json!({"safety": {"enabled": true}})
}

fn rule(settings: &Value, cwd: &str, command: &str) -> Option<String> {
    let decision = decision(settings, ROOT, cwd, command);
    match decision["decision"].as_str() {
        Some("allow") => None,
        Some("deny") => Some(decision["rule"].as_str().expect("rule").to_string()),
        other => panic!("decision {other:?}"),
    }
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
    let denied = decision(&settings, ROOT, ROOT, "terraform destroy");
    let reason = denied["reason"].as_str().expect("reason");
    assert!(
        reason.contains("rule `infra-destroy`: tearing down shared infrastructure"),
        "{reason}"
    );
}

#[test]
fn the_reason_names_the_rule_and_never_how_to_turn_it_off() {
    let denied = decision(&on(), ROOT, ROOT, "git push --force");
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
            ("discard-changes", "craft"),
            ("no-publish", "project"),
        ]
    );
    let off = super::listing(&json!({}));
    assert_eq!(
        (off["enabled"].clone(), off["origin"].clone()),
        (json!(false), json!("bundled"))
    );
}
