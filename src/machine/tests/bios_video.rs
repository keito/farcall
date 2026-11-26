use std::{
    fs::{self, OpenOptions},
    io::Write,
    sync::atomic::{AtomicU64, Ordering},
};

use crate::machine::{BootMode, Machine, MachineStepResult};

const VIDEO_BASE: usize = 0xB8000;
const VIDEO_END: usize = VIDEO_BASE + 0x4000;

fn machine_for_program(program: &[u8]) -> Machine {
    static NEXT_ID: AtomicU64 = AtomicU64::new(0);
    let path = std::env::temp_dir().join(format!(
        "farcall-bios-video-{}-{}.com",
        std::process::id(),
        NEXT_ID.fetch_add(1, Ordering::Relaxed)
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .unwrap();
    file.write_all(program).unwrap();
    drop(file);
    let machine = Machine::new(
        BootMode::Com {
            program: path.clone(),
        },
        0,
    );
    fs::remove_file(path).unwrap();
    machine.unwrap()
}

fn run_until_exit(machine: &mut Machine) {
    for _ in 0..128 {
        match machine.step().unwrap() {
            MachineStepResult::Continue => (),
            MachineStepResult::Exit(code) => {
                assert_eq!(code, 0);
                return;
            }
            MachineStepResult::WaitingForInput => panic!("video program waited for input"),
        }
    }
    panic!("video program did not terminate within 128 instructions");
}

#[test]
fn int10_mode06_query_reports_80_columns_and_page_zero() {
    let program = [
        0xB8, 0x06, 0x00, // MOV AX,0006h.
        0xCD, 0x10, // INT 10h selects mode 06h.
        0xB8, 0x5A, 0x0F, // MOV AX,0F5Ah seeds the previous AL value.
        0xBB, 0xCD, 0xAB, // MOV BX,ABCDh seeds the previous BH value.
        0xCD, 0x10, // INT 10h queries the current video mode.
        0xCD, 0x20, // INT 20h exits.
    ];
    for flags in [0x0242, 0x0CD7] {
        let mut machine = machine_for_program(&program);
        machine.cpu.flags = flags;
        machine.cpu.cx = 0x1234;
        machine.cpu.dx = 0x5678;
        run_until_exit(&mut machine);
        assert_eq!(machine.cpu.ax, 0x5006);
        assert_eq!(machine.cpu.bx, 0x00CD);
        assert_eq!((machine.cpu.cx, machine.cpu.dx), (0x1234, 0x5678));
        assert_eq!(machine.cpu.flags, flags);
    }
}

#[test]
fn int10_mode06_clears_the_entire_16k_video_buffer() {
    let mut machine = machine_for_program(&[
        0xB8, 0x06, 0x00, // MOV AX,0006h.
        0xCD, 0x10, // INT 10h.
        0xCD, 0x20, // INT 20h.
    ]);
    machine.memory[VIDEO_BASE..VIDEO_END].fill(0xA5);
    machine.memory[VIDEO_BASE - 1] = 0x5A;
    machine.memory[VIDEO_END] = 0x6B;
    run_until_exit(&mut machine);

    assert!(
        machine.memory[VIDEO_BASE..VIDEO_END]
            .iter()
            .all(|&byte| byte == 0)
    );
    assert_eq!(machine.memory[VIDEO_BASE - 1], 0x5A);
    assert_eq!(machine.memory[VIDEO_END], 0x6B);
    let mut frame = vec![0xFF; 640 * 200];
    machine.render_video(&mut frame);
    assert!(frame.iter().all(|&color| color == 0));
}

#[test]
fn int10_mode06_renders_guest_pixels_white_after_text_and_medium_resolution_modes() {
    for previous_mode in [0x03, 0x04, 0x05] {
        let mut program = vec![
            0xB8,
            previous_mode,
            0x00, // MOV AX,previous_mode.
            0xCD,
            0x10, // INT 10h.
        ];
        program.extend_from_slice(&[
            0xB8, 0x06, 0x00, // MOV AX,0006h.
            0xCD, 0x10, // INT 10h selects mode 06h.
            0xB8, 0x00, 0xB8, // MOV AX,B800h.
            0x8E, 0xC0, // MOV ES,AX.
            0x26, 0xC6, 0x06, 0x00, 0x00, 0x80, // MOV byte [ES:0000h],80h.
            0x26, 0xC6, 0x06, 0x00, 0x20, 0x01, // MOV byte [ES:2000h],01h.
            0x26, 0xC6, 0x06, 0x3F, 0x3F, 0x01, // MOV byte [ES:3F3Fh],01h.
            0xCD, 0x20, // INT 20h.
        ]);
        let mut machine = machine_for_program(&program);
        run_until_exit(&mut machine);

        let mut frame = vec![0xFF; 640 * 200];
        machine.render_video(&mut frame);
        let mut expected = vec![0; 640 * 200];
        for (x, y) in [(0, 0), (7, 1), (639, 199)] {
            expected[y * 640 + x] = 15;
        }
        assert_eq!(frame, expected, "previous mode {previous_mode:02X}");
    }
}

#[test]
fn int10_reselecting_mode06_clears_pixels_and_restores_white() {
    let mut machine = machine_for_program(&[
        0xB8, 0x06, 0x00, // MOV AX,0006h.
        0xCD, 0x10, // INT 10h.
        0xB8, 0x00, 0xB8, // MOV AX,B800h.
        0x8E, 0xC0, // MOV ES,AX.
        0x26, 0xC6, 0x06, 0x00, 0x00, 0x80, // MOV byte [ES:0000h],80h.
        0xBA, 0xD9, 0x03, // MOV DX,03D9h.
        0xB0, 0x04, // MOV AL,04h selects red.
        0xEE, // OUT DX,AL.
        0xB8, 0x06, 0x00, // MOV AX,0006h.
        0xCD, 0x10, // INT 10h reselects mode 06h.
        0x26, 0xC6, 0x06, 0x00, 0x20, 0x01, // MOV byte [ES:2000h],01h.
        0xCD, 0x20, // INT 20h.
    ]);
    // Stop after the guest draws the red pixel and before it reselects mode 06h.
    for _ in 0..8 {
        assert!(matches!(
            machine.step().unwrap(),
            MachineStepResult::Continue
        ));
    }
    let mut frame = vec![0xFF; 640 * 200];
    let mut expected = vec![0; 640 * 200];
    expected[0] = 4;
    machine.render_video(&mut frame);
    assert_eq!(frame, expected);

    run_until_exit(&mut machine);
    expected[0] = 0;
    expected[640 + 7] = 15;
    machine.render_video(&mut frame);
    assert_eq!(frame, expected);
}

#[test]
fn int10_can_return_from_mode06_to_text_mode() {
    let mut machine = machine_for_program(&[
        0xB8, 0x06, 0x00, // MOV AX,0006h.
        0xCD, 0x10, // INT 10h selects mode 06h.
        0xB8, 0x00, 0xB8, // MOV AX,B800h.
        0x8E, 0xC0, // MOV ES,AX.
        0x26, 0xC6, 0x06, 0x00, 0x00, 0xFF, // MOV byte [ES:0000h],FFh.
        0xB8, 0x03, 0x00, // MOV AX,0003h.
        0xCD, 0x10, // INT 10h restores text mode.
        0xB8, 0x00, 0x0F, // MOV AX,0F00h.
        0xBB, 0xCD, 0xAB, // MOV BX,ABCDh.
        0xCD, 0x10, // INT 10h queries text mode.
        0xCD, 0x20, // INT 20h.
    ]);
    let mut expected = vec![0xFF; 640 * 200];
    machine.render_video(&mut expected);
    run_until_exit(&mut machine);

    assert_eq!(machine.cpu.ax, 0x5003);
    assert_eq!(machine.cpu.bx, 0x00CD);
    for cell in machine.memory[VIDEO_BASE..VIDEO_BASE + 4000].chunks_exact(2) {
        assert_eq!(cell, b" \x07");
    }
    let mut frame = vec![0xFF; 640 * 200];
    machine.render_video(&mut frame);
    assert_eq!(frame, expected);
}
