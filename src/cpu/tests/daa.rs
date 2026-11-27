use super::{Cpu, CpuModel, CpuStepResult};

#[test]
fn daa_sets_decimal_carry_when_addition_did_not_set_binary_carry() {
    let mut cpu = Cpu::new(CpuModel::I8088, 0);
    cpu.ax = 0xA599; // AH sentinel; AL holds packed BCD 99.
    cpu.flags = 0xF702; // Set TF/IF/DF to check their preservation.
    [cpu.cx, cpu.dx, cpu.bx, cpu.sp, cpu.bp, cpu.si, cpu.di] =
        [0x1234, 0x2345, 0x3456, 0x4567, 0x5678, 0x6789, 0x789A];
    [cpu.ds, cpu.ss, cpu.es] = [0x2000, 0x3000, 0x4000];
    let other_registers = [
        cpu.cx, cpu.dx, cpu.bx, cpu.sp, cpu.bp, cpu.si, cpu.di, cpu.cs, cpu.ds, cpu.ss, cpu.es,
    ];
    let mut memory = [0x04, 0x01, 0x27]; // ADD AL, 1; DAA.

    assert!(matches!(
        cpu.step(&mut memory).unwrap(),
        CpuStepResult::Continue
    ));
    assert_eq!(cpu.ax, 0xA59A);
    assert_eq!(cpu.ip, 2);
    assert_eq!(cpu.flags & 0x0001, 0, "99h + 01h has no binary carry");
    assert_eq!(cpu.flags & 0x0010, 0, "9h + 1h has no nibble carry");

    assert!(matches!(
        cpu.step(&mut memory).unwrap(),
        CpuStepResult::Continue
    ));
    assert_eq!(cpu.ax, 0xA500, "DAA must write AL and preserve AH");
    assert_eq!(cpu.ip, 3, "DAA consumes only its opcode byte");
    assert_eq!(cpu.flags & 0x0001, 1, "decimal 99 + 1 must set carry");
    assert_eq!(
        cpu.flags & 0x00D4,
        0x0054,
        "AF, ZF and PF must be set; SF must be clear"
    );
    // OF is undefined after DAA; assert only control and reserved bits here.
    assert_eq!(cpu.flags & 0xF72A, 0xF702);
    assert_eq!(
        [
            cpu.cx, cpu.dx, cpu.bx, cpu.sp, cpu.bp, cpu.si, cpu.di, cpu.cs, cpu.ds, cpu.ss, cpu.es,
        ],
        other_registers
    );
    assert_eq!(memory, [0x04, 0x01, 0x27]);
}
