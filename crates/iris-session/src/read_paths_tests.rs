use super::*;
use std::fs;
use std::os::unix::fs::symlink;
use std::os::unix::net::UnixListener;

/// A folder for one test, outside `/tmp` (a folder of system state).
struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Dir {
        let p = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/read-paths-tests")
            .join(format!("{name}-{}", std::process::id()));
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
    let _l = UnixListener::bind(tool.join("lib/agent.sock")).unwrap();
    refused(check(&tool, &data, None), "socket");
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
    let (_d, _tool, data) = setup("home");
    // SAFETY: getpwuid and geteuid only read; one test thread reads it.
    let mine = homes(None).unwrap()[0].clone();
    if mine.exists() && mine != Path::new("/") {
        refused(check(&mine, &data, None), "home folder");
        if let Some(parent) = mine.parent().filter(|p| *p != Path::new("/")) {
            assert!(check(parent, &data, None).is_err(), "{}", parent.display());
        }
    }
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
    // /sys/fs/cgroup is a mount on most systems; the test needs one.
    let fs_dir = open_no_links(Path::new("/sys/fs")).unwrap();
    let cg = mount_at(fs_dir.as_raw_fd(), c"cgroup");
    let Ok(cg) = cg else { return };
    let own = mount_of(&fs_dir).unwrap();
    if cg == own {
        return;
    }
    let mut n = 0;
    let e = walk(&fs_dir, Path::new("/sys/fs"), &own, &mut n).unwrap_err();
    assert!(
        e.contains("mount point") || e.contains("cannot be read"),
        "{e}"
    );
}
