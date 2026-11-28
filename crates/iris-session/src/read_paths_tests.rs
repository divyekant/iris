use super::*;
use std::fs;
use std::os::unix::fs::symlink;
use std::os::unix::net::UnixListener;

/// A folder for one test, in a folder that the rules allow (`test_root`).
struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Dir {
        let p = test_root().join(format!("{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        Dir(fs::canonicalize(&p).unwrap())
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A tool folder with a file, a subfolder and a link out of the tree, and
/// a data folder beside it.
fn setup(name: &str) -> (Dir, PathBuf, PathBuf) {
    let d = Dir::new(name);
    let tool = d.0.join("tool");
    let data = d.0.join("data");
    fs::create_dir_all(tool.join("lib")).unwrap();
    fs::create_dir_all(&data).unwrap();
    fs::write(tool.join("lib/main.js"), "x").unwrap();
    symlink("/etc/hostname", tool.join("link")).unwrap();
    (d, tool, data)
}

fn refused(r: Result<Checked, String>, want: &str) {
    let e = r.expect_err("the read path must be refused");
    assert!(e.contains(want), "{e}");
}

#[test]
fn a_folder_of_files_passes_with_its_real_path() {
    let (d, tool, data) = setup("ok");
    let c = check(&tool, &data, None).unwrap();
    assert_eq!(c.real, tool);
    // A link to the folder gives the real path.
    symlink(&tool, d.0.join("alias")).unwrap();
    assert_eq!(check(&d.0.join("alias"), &data, None).unwrap().real, tool);
    // A single file passes too.
    let f = tool.join("lib/main.js");
    assert_eq!(check(&f, &data, None).unwrap().real, f);
}

#[test]
fn a_socket_or_a_fifo_in_the_tree_is_refused() {
    let (_d, tool, data) = setup("socket");
    let sock = tool.join("lib/agent.sock");
    // A socket path has at most 104 bytes on macOS. A checkout in a deep
    // folder (a git worktree) gets a longer test path: then only the FIFO
    // case runs. Linux CI has a short path.
    let short = sock.as_os_str().len() < 104;
    let _l = short.then(|| UnixListener::bind(&sock).unwrap());
    if short {
        refused(check(&tool, &data, None), "socket");
    }
    let (_d2, tool2, data2) = setup("fifo");
    let fifo = CString::new(tool2.join("pipe").as_os_str().as_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
    refused(check(&tool2, &data2, None), "FIFO");
}

#[test]
fn the_data_folder_is_refused_by_each_name() {
    let (d, tool, data) = setup("data");
    // The read path holds the data folder.
    refused(check(&d.0, &data, None), "data folder");
    // The read path is inside the data folder.
    fs::create_dir_all(data.join("content")).unwrap();
    refused(check(&data.join("content"), &data, None), "data folder");
    // A link to the data folder.
    symlink(&data, tool.join("lib/d")).unwrap();
    refused(check(&tool.join("lib/d"), &data, None), "data folder");
    // The link inside a read path stays a link and is not walked.
    assert!(check(&tool, &data, None).is_ok());
}

#[test]
fn folders_of_system_state_are_refused() {
    let (_d, _tool, data) = setup("state");
    for p in [
        "/proc",
        "/proc/self",
        "/dev",
        "/sys",
        "/run",
        "/tmp",
        "/var/tmp",
    ] {
        if Path::new(p).exists() {
            assert!(check(Path::new(p), &data, None).is_err(), "{p}");
        }
    }
    // A folder that holds one of them.
    assert!(check(Path::new("/var"), &data, None).is_err());
    assert!(check(Path::new("/"), &data, None).is_err());
}

#[test]
fn a_home_folder_is_refused_also_with_no_home_variable() {
    // The data folder must be outside the home folder. If not, the rule of
    // the data folder refuses first, as on a CI runner with the checkout in
    // the home folder.
    // create_dir fails on any path that exists (also a link), so the test
    // removes only the folder that it made.
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let made = std::env::temp_dir().join(format!(
        "iris-read-paths-home-{}-{nanos}",
        std::process::id()
    ));
    fs::create_dir(&made).unwrap();
    let data = fs::canonicalize(&made).unwrap();
    // SAFETY: getpwuid and geteuid only read; one test thread reads it.
    let mine = homes(None).unwrap()[0].clone();
    if mine.exists() && mine != Path::new("/") && !data.starts_with(&mine) {
        refused(check(&mine, &data, None), "home folder");
        if let Some(parent) = mine.parent().filter(|p| *p != Path::new("/")) {
            assert!(check(parent, &data, None).is_err(), "{}", parent.display());
        }
    }
    let _ = fs::remove_dir(&made);
    // HOME names one more home folder.
    let (_d2, tool, data2) = setup("home-env");
    refused(check(&tool, &data2, Some(tool.as_os_str())), "home folder");
}

#[test]
fn a_bind_mount_has_the_place_of_its_source() {
    let text = "\
22 1 8:1 / / rw - ext4 /dev/sda1 rw
40 22 8:1 /srv/iris /mnt/alias rw - ext4 /dev/sda1 rw
41 22 8:1 / /mnt/whole rw - ext4 /dev/sda1 rw
42 22 8:2 / /home/a\\040b rw shared:1 - ext4 /dev/sda2 rw
";
    let m = parse_mountinfo(text).unwrap();
    let data = m[0].place(Path::new("/srv/iris"));
    // A bind of the data folder at another name, with a root other than /.
    let alias = m[1].place(Path::new("/mnt/alias"));
    assert!(alias.holds(&data) && data.holds(&alias));
    // A bind of the whole file system, with the root /: it holds the data
    // folder.
    assert!(m[2].place(Path::new("/mnt/whole/x")).dev == data.dev);
    assert!(m[2].place(Path::new("/mnt/whole")).holds(&data));
    // An escape in the mount point, and another device.
    assert_eq!(m[3].point, Path::new("/home/a b"));
    assert!(!m[3].place(Path::new("/home/a b/srv/iris")).holds(&data));
}

#[test]
fn a_line_of_another_form_fails_the_parse() {
    for bad in [
        "22 1 8:1 / / rw ext4 /dev/sda1 rw\n",
        "x 1 8:1 / / rw - ext4 /dev/sda1 rw\n",
        "22 1 8:1 / /a\\9z rw - ext4 /dev/sda1 rw\n",
        "22 1 81 / / rw - ext4 /dev/sda1 rw\n",
        "22 1 8:1 relative / rw - ext4 /dev/sda1 rw\n",
    ] {
        assert!(parse_mountinfo(bad).is_none(), "{bad}");
    }
}

#[test]
fn a_nested_mount_is_refused_by_the_walk() {
    // /sys/fs/cgroup is a mount on most Linux systems; the test needs one.
    // macOS has no /sys.
    let Ok(fs_dir) = open_no_links(Path::new("/sys/fs")) else {
        return;
    };
    let cg = mount_at(fs_dir.as_raw_fd(), c"cgroup");
    let Ok(cg) = cg else { return };
    let own = mount_of(&fs_dir).unwrap();
    if cg == own {
        return;
    }
    let mut n = 0;
    let w = Walker {
        mount: own,
        linked: vec![],
    };
    let e = w.walk(&fs_dir, Path::new("/sys/fs"), &mut n).unwrap_err();
    assert!(
        e.contains("mount point") || e.contains("cannot be read"),
        "{e}"
    );
}

/// A small random number source for the property tests.
struct Rng(u64);

impl Rng {
    fn next(&mut self, n: u64) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 33) % n
    }

    /// An absolute path of random parts, with the bytes that mountinfo
    /// escapes (space, tab, newline, backslash).
    fn path(&mut self) -> Vec<u8> {
        // The file is read as UTF-8 (read_to_string), so the parts are
        // whole characters.
        let alphabet = ["a", "b", "-", "_", ".", "\\", " ", "\t", "\n", "é"];
        let mut p = Vec::new();
        for _ in 0..=self.next(4) {
            p.extend_from_slice(b"/");
            for _ in 0..=self.next(6) {
                let c = alphabet[self.next(alphabet.len() as u64) as usize];
                p.extend_from_slice(c.as_bytes());
            }
        }
        p
    }
}

/// The escape of the kernel for a path in mountinfo.
fn escape(p: &[u8]) -> String {
    let mut out = Vec::new();
    for &b in p {
        match b {
            b' ' | b'\t' | b'\n' | b'\\' => out.extend(format!("\\{b:03o}").bytes()),
            _ => out.push(b),
        }
    }
    String::from_utf8(out).unwrap()
}

#[test]
fn mountinfo_lines_round_trip_and_places_follow_the_root() {
    let mut r = Rng(0x5eed);
    for _ in 0..2000 {
        let (root, point, rest) = (r.path(), r.path(), r.path());
        let optional: Vec<String> = (0..r.next(3)).map(|i| format!("shared:{i}")).collect();
        let id = r.next(1 << 20);
        let line = format!(
            "{id} 1 {}:{} {} {} rw{}{} - ext4 /dev/x rw\n",
            r.next(300),
            r.next(300),
            escape(&root),
            escape(&point),
            if optional.is_empty() { "" } else { " " },
            optional.join(" ")
        );
        let m = parse_mountinfo(&line).unwrap_or_else(|| panic!("{line:?}"));
        let m = &m[0];
        assert_eq!(m.id, id);
        assert_eq!(m.root.as_os_str().as_bytes(), &root[..]);
        assert_eq!(m.point.as_os_str().as_bytes(), &point[..]);
        // A path below the mount point has the place root + rest.
        let mut real = point.clone();
        real.extend_from_slice(&rest);
        let place = m.place(Path::new(OsStr::from_bytes(&real)));
        let mut want = root.clone();
        want.extend_from_slice(&rest);
        assert_eq!(place.path, Path::new(OsStr::from_bytes(&want)), "{line:?}");
    }
}

#[test]
fn a_changed_mountinfo_line_parses_safely_or_not_at_all() {
    let mut r = Rng(0xfeed);
    let base = b"36 35 98:0 /mnt1 /mnt/parent rw,noatime master:1 - ext3 /dev/root rw\n";
    for _ in 0..20000 {
        let mut b = base.to_vec();
        for _ in 0..=r.next(4) {
            let i = r.next(b.len() as u64) as usize;
            match r.next(3) {
                0 => b[i] = r.next(256) as u8,
                1 => {
                    b.remove(i);
                }
                _ => b.insert(i, b" \\-:/0"[r.next(6) as usize]),
            }
        }
        let text = String::from_utf8_lossy(&b);
        // No panic, and each parsed mount has absolute paths and a device.
        if let Some(ms) = parse_mountinfo(&text) {
            for m in ms {
                assert!(m.root.is_absolute() && m.point.is_absolute(), "{text:?}");
                assert!(m.dev.contains(':'), "{text:?}");
            }
        }
    }
}

#[test]
fn an_object_moved_to_the_checked_name_is_refused() {
    let (d, tool, data) = setup("moved");
    // The object opened at a name, then moved: the data of the key now
    // stands at another name.
    let key = data.join("key");
    fs::write(&key, "secret").unwrap();
    let fd = open_no_links(&key).unwrap();
    let other = d.0.join("tool-file");
    fs::rename(&key, &other).unwrap();
    assert_eq!(path_of(&fd).unwrap(), other);
    refused(
        check_open(&tool, tool.clone(), fd, &data, None),
        "changed while Iris checked",
    );
    // A file opened and checked at its own name passes.
    let f = tool.join("lib/main.js");
    let fd = open_no_links(&f).unwrap();
    assert!(check_open(&f, f.clone(), fd, &data, None).is_ok());
}

#[test]
fn a_hard_link_to_a_file_of_the_data_folder_is_refused() {
    let (_d, tool, data) = setup("hardlink");
    fs::write(data.join("key"), "secret").unwrap();
    fs::hard_link(data.join("key"), tool.join("lib/key")).unwrap();
    refused(check(&tool, &data, None), "hard link");
    // The read path that names the linked file.
    refused(check(&tool.join("lib/key"), &data, None), "hard link");
    // A file with one link passes.
    fs::remove_file(tool.join("lib/key")).unwrap();
    assert!(check(&tool, &data, None).is_ok());
}

#[test]
fn the_test_folder_leaves_a_checkout_in_a_folder_of_system_state() {
    let home = Path::new("/home/a");
    assert_eq!(
        test_root_of(Path::new("/tmp/r/iris/target"), home),
        Path::new("/home/a/.cache/iris-read-paths-tests")
    );
    assert_eq!(
        test_root_of(Path::new("/srv/iris/target"), home),
        Path::new("/srv/iris/target/read-paths-tests")
    );
}
