mod callee {
    use anyhow::{bail, Context, Result};
    use itertools::Itertools;

    use std::io::{BufRead, Write};
    use std::path::{Path, PathBuf};

    pub trait Handler {
        fn detect(&mut self, path: &Path) -> Result<bool>;
        fn gather(&mut self, path: &Path, shadow_root: &Path) -> Result<()>;
        fn affect(&mut self, path: &Path, shadow_root: &Path) -> Result<()>;
    }

    pub fn parse_and_dispatch(
        instream: &mut impl BufRead,
        outstream: &mut impl Write,
        handler: &mut impl Handler,
    ) -> Result<()> {
        let mut buf = String::new();
        loop {
            buf.clear();
            if instream.read_line(&mut buf)? == 0 {
                return Ok(()); // EOF
            }
            if buf.ends_with("\n") {
                buf.pop();
            }

            if &buf == "com.akavel.mana.v2.rq" {
                writeln!(outstream, "com.akavel.mana.v2.rs")?;
                continue;
            }
            const DETECT: &str = "detect ";
            if let Some(raw_args) = buf.strip_prefix(DETECT) {
                // TODO: split on space, verify nothing after it
                // TODO: urlencoding lib looks not super stable, use better one
                let Some((raw_arg1,)) = raw_args.split(" ").collect_tuple() else {
                    bail!("expected exactly 1 arg to 'detect', got: {raw_args:?}");
                };
                let path1 = PathBuf::from(urlencoding::decode(raw_arg1)?.as_ref());
                let found = handler.detect(&path1)?;
                let answer = if found { "present" } else { "absent" };
                writeln!(outstream, "detected {answer}")?;
                continue;
            }
            const GATHER: &str = "gather ";
            if let Some(raw_args) = buf.strip_prefix(GATHER) {
                // TODO: more details in error handling
                let Some((raw_arg1, raw_arg2)) = raw_args.split(" ").collect_tuple() else {
                    bail!("expected exactly 2 args to 'gather', got: {raw_args:?}");
                };
                let path1 = PathBuf::from(urlencoding::decode(raw_arg1)?.as_ref());
                let path2 = PathBuf::from(urlencoding::decode(raw_arg2)?.as_ref());
                handler.gather(&path1, &path2)?;
                writeln!(outstream, "gathered {raw_arg1} {raw_arg2}")?;
                continue;
            }
            const AFFECT: &str = "affect ";
            if let Some(raw_args) = buf.strip_prefix(AFFECT) {
                // TODO: more details in error handling
                let Some((raw_arg1, raw_arg2)) = raw_args.split(" ").collect_tuple() else {
                    bail!("expected exactly 2 args to 'affect', got: {raw_args:?}");
                };
                let path1 = PathBuf::from(urlencoding::decode(raw_arg1)?.as_ref());
                let path2 = PathBuf::from(urlencoding::decode(raw_arg2)?.as_ref());
                handler.affect(&path1, &path2)?;
                writeln!(outstream, "affected {raw_arg1} {raw_arg2}")?;
                continue;
            }
            bail!("unknown format of input line: {buf:?}");
        }
    }
}

mod caller {
    use std::process::*;

    pub struct Cmd {
        stdin: ChildStdin,
        //stdout: ChildStdout,
        stdout: std::io::BufReader<ChildStdout>,
        //stderr: ChildStderr, // TODO[LATER]
    }

    impl Cmd {
        pub fn detect(&mut self, path: &Path) -> Result<bool> {
            Ok(self.call("detect", path, None)? == "present".to_string())
        }

        pub fn gather(&mut self, path: &Path, shadow_root: &Path) -> Result<()> {
            Ok(self.call("gather", path, Some(shadow_root))?)
        }

        pub fn affect(&mut self, path: &Path, shadow_root: &Path) -> Result<()> {
            Ok(self.call("affect", path, Some(shadow_root))?)
        }

        fn call(&mut self, cmd: &str, path: &Path, shadow_root: Option<&Path>) -> Result<String> {
            // TODO[LATER]: spawn thread to ensure we don't deadlock if pipe buffer size exceeded
            let path_enc = urlencoding::encode(path);
            if let Some(shadow) = shadow_root {
                let shadow_enc = urlencoding::encode(shadow);
                writeln!(self.stdin, "{cmd} {path_enc} {shadow_enc}")?;
            } else {
                writeln!(self.stdin, "{cmd} {path_enc}")?;
            }
            let mut buf = String::new();
            self.stdout.read_line(&mut buf)?;
            let prefix = cmd.to_string() + "ed ";
            let Some(suffix) = buf.strip_prefix(prefix) else {
                // TODO[LATER]: print name of called command
                bail!("bad mana protocol response to {cmd}: {buf:?}");
            };
            Ok(suffix.trim_end_matches("\n").to_string())
        }
    }
}

#[cfg(test)]
mod test {
    use super::callee::{parse_and_dispatch, Handler};
    use anyhow::{bail, Context, Result};
    use std::io::BufReader;
    use std::path::Path;

    struct TestHandler {
        lines: Vec<(String, String, String)>,
        last_detect: bool,
    }

    impl Default for TestHandler {
        fn default() -> Self {
            Self {
                lines: vec![],
                last_detect: false,
            }
        }
    }

    impl Handler for TestHandler {
        fn detect(&mut self, path: &Path) -> Result<bool> {
            self.lines.push((
                "detect".to_string(),
                path.to_string_lossy().into_owned(),
                "".to_string(),
            ));
            self.last_detect = !self.last_detect;
            Ok(self.last_detect)
        }

        fn gather(&mut self, path: &Path, shadow_root: &Path) -> Result<()> {
            self.lines.push((
                "gather".to_string(),
                path.to_string_lossy().into_owned(),
                shadow_root.to_string_lossy().into_owned(),
            ));
            Ok(())
        }

        fn affect(&mut self, path: &Path, shadow_root: &Path) -> Result<()> {
            self.lines.push((
                "affect".to_string(),
                path.to_string_lossy().into_owned(),
                shadow_root.to_string_lossy().into_owned(),
            ));
            Ok(())
        }
    }

    #[test]
    fn parsing_and_dispatching() {
        let mut script = r#"com.akavel.mana.v2.rq
detect foo/bar/baz
detect fee/fo/fum
gather bee/bop zee/zam
affect a/b c/d
"#
        .as_bytes();
        let mut h = TestHandler::default();
        let mut buf = Vec::new();
        parse_and_dispatch(&mut script, &mut buf, &mut h).unwrap();
        assert_eq!(
            h.lines,
            vec![
                (
                    "detect".to_string(),
                    "foo/bar/baz".to_string(),
                    "".to_string()
                ),
                (
                    "detect".to_string(),
                    "fee/fo/fum".to_string(),
                    "".to_string()
                ),
                (
                    "gather".to_string(),
                    "bee/bop".to_string(),
                    "zee/zam".to_string(),
                ),
                ("affect".to_string(), "a/b".to_string(), "c/d".to_string(),),
            ]
        );
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            r#"com.akavel.mana.v2.rs
detected present
detected absent
gathered bee/bop zee/zam
affected a/b c/d
"#
        );
    }
}
