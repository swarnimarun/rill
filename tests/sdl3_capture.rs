#![cfg(all(unix, feature = "c-import"))]

use std::{fs, path::Path, process::Command};

struct Directory(std::path::PathBuf);

impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn real_sdl3_framebuffers_match_at_o0_and_o2() {
    for (tool, args) in [
        ("pkg-config", vec!["--exists", "sdl3"]),
        ("python3", vec!["--version"]),
        ("bash", vec!["--version"]),
    ] {
        if !Command::new(tool)
            .args(args)
            .output()
            .is_ok_and(|output| output.status.success())
        {
            eprintln!("SKIP SDL3 framebuffer test: {tool} and SDL3 development files required");
            return;
        }
    }
    let dir =
        Directory(std::env::temp_dir().join(format!("rill SDL3 capture {}", std::process::id())));
    fs::create_dir(&dir.0).unwrap();
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/capture-sdl3.sh");
    for opt in ["0", "2"] {
        let output_dir = dir.0.join(format!("optimization {opt}"));
        let output = Command::new("bash")
            .arg(&script)
            .arg(&output_dir)
            .env("RILL_BIN", env!("CARGO_BIN_EXE_rill"))
            .env("RILL_CAPTURE_OPT_LEVEL", opt)
            .env("RILL_CAPTURE_FRAME", "40")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "capture at -O{opt} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        for game in ["direct", "collector"] {
            assert_eq!(
                fs::read(output_dir.join(format!("{game}.stdout"))).unwrap(),
                b"0\n"
            );
            let bytes = fs::read(output_dir.join(format!("sdl3-{game}.bmp"))).unwrap();
            let expected_height = if game == "direct" { 400 } else { 480 };
            assert_eq!(le_u32(&bytes, 18), 640);
            assert_eq!(le_u32(&bytes, 22), expected_height);
            if game == "direct" {
                assert_eq!(pixel(&bytes, 0, 0), [20, 24, 32]);
                assert_eq!(pixel(&bytes, 280, 200), [70, 210, 140]);
                assert_eq!(pixel(&bytes, 360, 200), [70, 210, 140]);
                assert_eq!(pixel(&bytes, 313, 200), [250, 190, 60]);
                assert_eq!(pixel(&bytes, 40, 350), [20, 24, 32]);
            } else {
                assert_eq!(pixel(&bytes, 0, 0), [18, 22, 32]);
                assert_eq!(pixel(&bytes, 310, 230), [80, 165, 250]);
                assert_eq!(pixel(&bytes, 110, 110), [242, 190, 58]);
            }
            if opt == "2" {
                assert_eq!(
                    bytes,
                    fs::read(dir.0.join(format!("optimization 0/sdl3-{game}.bmp"))).unwrap(),
                    "{game} framebuffer differs between -O0 and -O2"
                );
            }
        }
        let before = fs::read(output_dir.join("sdl3-direct.bmp")).unwrap();
        let repeated = Command::new("bash")
            .arg(&script)
            .arg(&output_dir)
            .output()
            .unwrap();
        assert!(!repeated.status.success());
        assert!(String::from_utf8_lossy(&repeated.stderr).contains("Refusing to overwrite"));
        assert_eq!(
            before,
            fs::read(output_dir.join("sdl3-direct.bmp")).unwrap()
        );
    }
}

fn le_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn pixel(bytes: &[u8], x: usize, y: usize) -> [u8; 3] {
    assert_eq!(&bytes[..2], b"BM");
    assert_eq!(u16::from_le_bytes(bytes[28..30].try_into().unwrap()), 24);
    assert_eq!(le_u32(bytes, 30), 0, "expected uncompressed RGB BMP");
    let width = le_u32(bytes, 18) as usize;
    let height = le_u32(bytes, 22) as usize;
    let stride = (width * 3).div_ceil(4) * 4;
    let offset = le_u32(bytes, 10) as usize + (height - 1 - y) * stride + x * 3;
    [bytes[offset + 2], bytes[offset + 1], bytes[offset]]
}
