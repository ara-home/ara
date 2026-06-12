use ara_types::RiskLevel;

pub struct Pattern {
    pub id: &'static str,
    pub severity: RiskLevel,
    pub regex: &'static str,
    pub file_glob: &'static str,
    pub description: &'static str,
}

const GLOB: &str = "*.{js,ts,jsx,tsx,mjs,cjs,mts,cts}";

#[allow(clippy::too_many_lines)]
pub const fn all_patterns() -> &'static [Pattern] {
    &[
        Pattern {
            id: "eval-usage",
            severity: RiskLevel::Critical,
            regex: r"\beval\s*\(",
            file_glob: GLOB,
            description: "eval() allows arbitrary code execution",
        },
        Pattern {
            id: "new-function",
            severity: RiskLevel::Critical,
            regex: r"new\s+Function\s*\(",
            file_glob: GLOB,
            description: "new Function() creates dynamically executed code",
        },
        Pattern {
            id: "child-process-exec",
            severity: RiskLevel::High,
            regex: r"(?:\.\s*)?(?:spawn|spawnSync|fork|execFile|execFileSync|execSync)\s*\(|(?:^|[^.\w])exec\s*\(",
            file_glob: GLOB,
            description: "Child process execution — potential shell injection",
        },
        Pattern {
            id: "child-process-require",
            severity: RiskLevel::High,
            regex: r#"require\s*\(\s*['"`]child_process['"`]\s*\)"#,
            file_glob: GLOB,
            description: "Import of child_process module",
        },
        Pattern {
            id: "vm-escape",
            severity: RiskLevel::High,
            regex: r"vm\.\s*(?:runInThisContext|runInNewContext|compileFunction|createScript)\s*\(",
            file_glob: GLOB,
            description: "VM sandbox escape method detected",
        },
        Pattern {
            id: "process-binding",
            severity: RiskLevel::High,
            regex: r"process\.\s*binding\s*\(",
            file_glob: GLOB,
            description: "process.binding() provides access to native addons",
        },
        Pattern {
            id: "prototype-pollution",
            severity: RiskLevel::High,
            regex: r"__proto__[^=:]*[=:]",
            file_glob: GLOB,
            description: "Prototype pollution via __proto__ assignment",
        },
        Pattern {
            id: "constructor-pollution",
            severity: RiskLevel::High,
            regex: r"\.constructor\s*\.\s*prototype",
            file_glob: GLOB,
            description: "Prototype pollution via constructor.prototype",
        },
        Pattern {
            id: "fs-dangerous-write",
            severity: RiskLevel::Medium,
            regex: r"fs\.\s*(?:writeFile|writeFileSync|appendFile|appendFileSync)\s*\(",
            file_glob: GLOB,
            description: "File system write operation",
        },
        Pattern {
            id: "fs-dangerous-delete",
            severity: RiskLevel::Medium,
            regex: r"fs\.\s*(?:unlink|unlinkSync|rm|rmSync|rmdir|rmdirSync)\s*\(",
            file_glob: GLOB,
            description: "File system delete operation",
        },
        Pattern {
            id: "credential-access",
            severity: RiskLevel::Medium,
            regex: r"process\.env\.\s*(?:NODE_|AWS_|GITHUB_|TOKEN|SECRET|PASSWORD|PASS|API_KEY|API_SECRET|ACCESS_KEY|SECRET_KEY|PRIVATE_KEY)",
            file_glob: GLOB,
            description: "Access to environment credentials or secrets",
        },
        Pattern {
            id: "alloc-unsafe",
            severity: RiskLevel::Medium,
            regex: r"Buffer\.\s*(?:allocUnsafe|allocUnsafeSlow)\s*\(",
            file_glob: GLOB,
            description: "Uninitialized memory allocation — may leak sensitive data",
        },
        Pattern {
            id: "dynamic-require",
            severity: RiskLevel::Medium,
            regex: r#"require\s*\(\s*[^'"`\s)]"#,
            file_glob: GLOB,
            description: "Dynamic require() with non-string argument",
        },
        Pattern {
            id: "dynamic-import",
            severity: RiskLevel::Medium,
            regex: r#"import\s*\(\s*[^'"`\s)]"#,
            file_glob: GLOB,
            description: "Dynamic import() with non-string argument",
        },
        Pattern {
            id: "weak-crypto",
            severity: RiskLevel::Medium,
            regex: r#"createHash\s*\(\s*['"`](?:md5|sha1|ripemd160)['"`]"#,
            file_glob: GLOB,
            description: "Use of weak hashing algorithm (MD5, SHA-1, RIPEMD-160)",
        },
        Pattern {
            id: "math-random",
            severity: RiskLevel::Low,
            regex: r"Math\.random\s*\(\s*\)",
            file_glob: GLOB,
            description: "Math.random() is not cryptographically secure",
        },
        Pattern {
            id: "deprecated-cipher",
            severity: RiskLevel::Low,
            regex: r"(?:createCipher|createDecipher|createDecipheriv)\s*\(",
            file_glob: GLOB,
            description: "Use of deprecated Node.js cipher methods",
        },
    ]
}

pub const fn install_scripts_pattern() -> Pattern {
    Pattern {
        id: "install-scripts",
        severity: RiskLevel::High,
        regex: "",
        file_glob: "package.json",
        description: "Package defines preinstall/install/postinstall scripts",
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    struct PatternCase {
        code: &'static str,
        should_match: bool,
    }

    fn assert_pattern(pattern: &Pattern, cases: &[PatternCase]) {
        let re = regex::Regex::new(pattern.regex).unwrap();
        for case in cases {
            let matched = re.is_match(case.code);
            assert_eq!(
                matched,
                case.should_match,
                "pattern `{}` {} on: {:?}",
                pattern.id,
                if case.should_match {
                    "should match"
                } else {
                    "should NOT match"
                },
                case.code,
            );
        }
    }

    #[test]
    fn test_eval_usage() {
        let p = &all_patterns()[0];
        assert_eq!(p.id, "eval-usage");
        assert_pattern(
            p,
            &[
                PatternCase {
                    code: "eval(code)",
                    should_match: true,
                },
                PatternCase {
                    code: "  eval( payload )",
                    should_match: true,
                },
                PatternCase {
                    code: "window.eval(code)",
                    should_match: true,
                },
                PatternCase {
                    code: "evaluate()",
                    should_match: false,
                },
                PatternCase {
                    code: "uneval(obj)",
                    should_match: false,
                },
                PatternCase {
                    code: "// eval is dangerous",
                    should_match: false,
                },
            ],
        );
    }

    #[test]
    fn test_new_function() {
        let p = &all_patterns()[1];
        assert_eq!(p.id, "new-function");
        assert_pattern(
            p,
            &[
                PatternCase {
                    code: "new Function('return x')",
                    should_match: true,
                },
                PatternCase {
                    code: "new Function(a, b, body)",
                    should_match: true,
                },
                PatternCase {
                    code: "new function() {}",
                    should_match: false,
                },
                PatternCase {
                    code: "Function.prototype",
                    should_match: false,
                },
            ],
        );
    }

    #[test]
    fn test_child_process_exec() {
        let p = &all_patterns()[2];
        assert_eq!(p.id, "child-process-exec");
        assert_pattern(
            p,
            &[
                PatternCase {
                    code: "cp.exec(cmd)",
                    should_match: false,
                },
                PatternCase {
                    code: "child_process.execSync('ls')",
                    should_match: true,
                },
                PatternCase {
                    code: "spawn('bash', args)",
                    should_match: true,
                },
                PatternCase {
                    code: "cp.fork('child.js')",
                    should_match: true,
                },
                PatternCase {
                    code: "exec(cmd)",
                    should_match: true,
                },
                PatternCase {
                    code: "re.exec(str)",
                    should_match: false,
                },
                PatternCase {
                    code: "execute()",
                    should_match: false,
                },
            ],
        );
    }

    #[test]
    fn test_child_process_require() {
        let p = &all_patterns()[3];
        assert_eq!(p.id, "child-process-require");
        assert_pattern(
            p,
            &[
                PatternCase {
                    code: "require('child_process')",
                    should_match: true,
                },
                PatternCase {
                    code: "require(\"child_process\")",
                    should_match: true,
                },
                PatternCase {
                    code: "require(`child_process`)",
                    should_match: true,
                },
                PatternCase {
                    code: "require('fs')",
                    should_match: false,
                },
                PatternCase {
                    code: "require('child_process').exec",
                    should_match: true,
                },
            ],
        );
    }

    #[test]
    fn test_vm_escape() {
        let p = &all_patterns()[4];
        assert_eq!(p.id, "vm-escape");
        assert_pattern(
            p,
            &[
                PatternCase {
                    code: "vm.runInThisContext(code)",
                    should_match: true,
                },
                PatternCase {
                    code: "vm.runInNewContext(code, ctx)",
                    should_match: true,
                },
                PatternCase {
                    code: "vm.compileFunction('return 1')",
                    should_match: true,
                },
                PatternCase {
                    code: "vm.createScript(src)",
                    should_match: true,
                },
                PatternCase {
                    code: "vm.run(code)",
                    should_match: false,
                },
            ],
        );
    }

    #[test]
    fn test_process_binding() {
        let p = &all_patterns()[5];
        assert_eq!(p.id, "process-binding");
        assert_pattern(
            p,
            &[
                PatternCase {
                    code: "process.binding('spawn_sync')",
                    should_match: true,
                },
                PatternCase {
                    code: "process.binding('natives')",
                    should_match: true,
                },
                PatternCase {
                    code: "process.pid",
                    should_match: false,
                },
                PatternCase {
                    code: "process.env",
                    should_match: false,
                },
            ],
        );
    }

    #[test]
    fn test_prototype_pollution() {
        let p = &all_patterns()[6];
        assert_eq!(p.id, "prototype-pollution");
        assert_pattern(
            p,
            &[
                PatternCase {
                    code: "obj.__proto__ = x",
                    should_match: true,
                },
                PatternCase {
                    code: "\"__proto__\": value",
                    should_match: true,
                },
                PatternCase {
                    code: "obj.__defineGetter__",
                    should_match: false,
                },
            ],
        );
    }

    #[test]
    fn test_constructor_pollution() {
        let p = &all_patterns()[7];
        assert_eq!(p.id, "constructor-pollution");
        assert_pattern(
            p,
            &[
                PatternCase {
                    code: "obj.constructor.prototype",
                    should_match: true,
                },
                PatternCase {
                    code: "a.constructor.prototype.pollute",
                    should_match: true,
                },
                PatternCase {
                    code: "obj.constructor",
                    should_match: false,
                },
            ],
        );
    }

    #[test]
    fn test_fs_dangerous_write() {
        let p = &all_patterns()[8];
        assert_eq!(p.id, "fs-dangerous-write");
        assert_pattern(
            p,
            &[
                PatternCase {
                    code: "fs.writeFile(path, data)",
                    should_match: true,
                },
                PatternCase {
                    code: "fs.writeFileSync(path, data)",
                    should_match: true,
                },
                PatternCase {
                    code: "fs.appendFile(path, data)",
                    should_match: true,
                },
                PatternCase {
                    code: "fs.readFile(path)",
                    should_match: false,
                },
            ],
        );
    }

    #[test]
    fn test_fs_dangerous_delete() {
        let p = &all_patterns()[9];
        assert_eq!(p.id, "fs-dangerous-delete");
        assert_pattern(
            p,
            &[
                PatternCase {
                    code: "fs.unlink(path)",
                    should_match: true,
                },
                PatternCase {
                    code: "fs.rmSync(path)",
                    should_match: true,
                },
                PatternCase {
                    code: "fs.rmdir(path)",
                    should_match: true,
                },
                PatternCase {
                    code: "fs.mkdir(path)",
                    should_match: false,
                },
            ],
        );
    }

    #[test]
    fn test_credential_access() {
        let p = &all_patterns()[10];
        assert_eq!(p.id, "credential-access");
        assert_pattern(
            p,
            &[
                PatternCase {
                    code: "process.env.NODE_ENV",
                    should_match: true,
                },
                PatternCase {
                    code: "process.env.AWS_SECRET_KEY",
                    should_match: true,
                },
                PatternCase {
                    code: "process.env.GITHUB_TOKEN",
                    should_match: true,
                },
                PatternCase {
                    code: "process.env.API_KEY",
                    should_match: true,
                },
                PatternCase {
                    code: "process.env.HOME",
                    should_match: false,
                },
                PatternCase {
                    code: "process.env.PATH",
                    should_match: false,
                },
            ],
        );
    }

    #[test]
    fn test_alloc_unsafe() {
        let p = &all_patterns()[11];
        assert_eq!(p.id, "alloc-unsafe");
        assert_pattern(
            p,
            &[
                PatternCase {
                    code: "Buffer.allocUnsafe(1024)",
                    should_match: true,
                },
                PatternCase {
                    code: "Buffer.allocUnsafeSlow(size)",
                    should_match: true,
                },
                PatternCase {
                    code: "Buffer.alloc(1024)",
                    should_match: false,
                },
                PatternCase {
                    code: "Buffer.from(data)",
                    should_match: false,
                },
            ],
        );
    }

    #[test]
    fn test_dynamic_require() {
        let p = &all_patterns()[12];
        assert_eq!(p.id, "dynamic-require");
        assert_pattern(
            p,
            &[
                PatternCase {
                    code: "require(variable)",
                    should_match: true,
                },
                PatternCase {
                    code: "require( name )",
                    should_match: true,
                },
                PatternCase {
                    code: "require('fs')",
                    should_match: false,
                },
                PatternCase {
                    code: "require(\"fs\")",
                    should_match: false,
                },
                PatternCase {
                    code: "require(`fs`)",
                    should_match: false,
                },
            ],
        );
    }

    #[test]
    fn test_dynamic_import() {
        let p = &all_patterns()[13];
        assert_eq!(p.id, "dynamic-import");
        assert_pattern(
            p,
            &[
                PatternCase {
                    code: "import(variable)",
                    should_match: true,
                },
                PatternCase {
                    code: "import( module )",
                    should_match: true,
                },
                PatternCase {
                    code: "import('fs')",
                    should_match: false,
                },
                PatternCase {
                    code: "import(\"fs\")",
                    should_match: false,
                },
                PatternCase {
                    code: "import(`fs`)",
                    should_match: false,
                },
            ],
        );
    }

    #[test]
    fn test_weak_crypto() {
        let p = &all_patterns()[14];
        assert_eq!(p.id, "weak-crypto");
        assert_pattern(
            p,
            &[
                PatternCase {
                    code: "createHash('md5')",
                    should_match: true,
                },
                PatternCase {
                    code: "createHash(\"sha1\")",
                    should_match: true,
                },
                PatternCase {
                    code: "createHash(`ripemd160`)",
                    should_match: true,
                },
                PatternCase {
                    code: "createHash('sha256')",
                    should_match: false,
                },
                PatternCase {
                    code: "createHash('sha512')",
                    should_match: false,
                },
                PatternCase {
                    code: "createHmac('md5', key)",
                    should_match: false,
                },
            ],
        );
    }

    #[test]
    fn test_math_random() {
        let p = &all_patterns()[15];
        assert_eq!(p.id, "math-random");
        assert_pattern(
            p,
            &[
                PatternCase {
                    code: "Math.random()",
                    should_match: true,
                },
                PatternCase {
                    code: "Math.random( )",
                    should_match: true,
                },
                PatternCase {
                    code: "Math.floor(Math.random() * 10)",
                    should_match: true,
                },
                PatternCase {
                    code: "Math.PI",
                    should_match: false,
                },
                PatternCase {
                    code: "crypto.randomBytes(16)",
                    should_match: false,
                },
            ],
        );
    }

    #[test]
    fn test_deprecated_cipher() {
        let p = &all_patterns()[16];
        assert_eq!(p.id, "deprecated-cipher");
        assert_pattern(
            p,
            &[
                PatternCase {
                    code: "createCipher('aes-128-cbc', key)",
                    should_match: true,
                },
                PatternCase {
                    code: "createDecipher('aes-256-cbc', key)",
                    should_match: true,
                },
                PatternCase {
                    code: "createDecipheriv(alg, key, iv)",
                    should_match: true,
                },
                PatternCase {
                    code: "createCipheriv('aes-256-gcm', key, iv)",
                    should_match: false,
                },
            ],
        );
    }

    #[test]
    fn test_all_patterns_count() {
        assert_eq!(all_patterns().len(), 17);
    }

    #[test]
    fn test_all_patterns_have_unique_ids() {
        let mut ids: Vec<&str> = all_patterns().iter().map(|p| p.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), all_patterns().len());
    }

    #[test]
    fn test_install_scripts_pattern() {
        let p = install_scripts_pattern();
        assert_eq!(p.id, "install-scripts");
        assert_eq!(p.severity, RiskLevel::High);
        assert_eq!(p.file_glob, "package.json");
    }
}
