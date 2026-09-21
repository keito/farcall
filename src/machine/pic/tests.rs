use super::Pic;

// These tests cover the current fixed-priority, edge-triggered model with
// explicit EOI. Configure IMR directly until PIC port handling is implemented.

#[test]
fn new_pic_has_no_pending_interrupt_and_idle_eoi_is_harmless() {
    let mut pic = Pic::new();
    assert!(!pic.interrupt_pending());
    pic.end_interrupt(None);
    for irq in 0..8 {
        pic.set_irq_line(irq, false);
        pic.end_interrupt(Some(irq));
    }
    assert!(!pic.interrupt_pending());
    assert_eq!(pic.isr, 0);
}

#[test]
fn each_irq_maps_to_its_pc_vector_and_polling_does_not_consume_it() {
    for (irq, vector) in (0..8).zip(0x08..=0x0F) {
        let mut pic = Pic::new();
        pic.set_irq_line(irq, true);
        for _ in 0..3 {
            assert!(pic.interrupt_pending());
        }
        assert_eq!(pic.acknowledge().unwrap(), vector);
        assert_eq!(pic.irr, 0);
        assert_eq!(pic.isr, 1 << irq);
        assert!(!pic.interrupt_pending());
    }
}

#[test]
fn simultaneous_requests_are_serviced_in_priority_order_without_loss() {
    let mut pic = Pic::new();
    for irq in (0..8).rev() {
        pic.set_irq_line(irq, true);
    }
    for vector in 0x08..=0x0F {
        assert!(pic.interrupt_pending());
        assert_eq!(pic.acknowledge().unwrap(), vector);
        assert!(
            !pic.interrupt_pending(),
            "lower priorities must wait for EOI"
        );
        pic.end_interrupt(None);
    }
    assert!(!pic.interrupt_pending());
}

#[test]
fn held_high_line_does_not_retrigger_until_a_new_rising_edge() {
    for irq in 0..8 {
        let mut pic = Pic::new();
        pic.set_irq_line(irq, true);
        pic.acknowledge().unwrap();
        pic.end_interrupt(Some(irq));
        for _ in 0..3 {
            pic.set_irq_line(irq, true);
            assert!(!pic.interrupt_pending());
        }
        pic.set_irq_line(irq, false);
        assert!(!pic.interrupt_pending());
        pic.set_irq_line(irq, true);
        assert!(pic.interrupt_pending());
        assert_eq!(pic.acknowledge().unwrap(), 0x08 + irq);
    }
}

#[test]
fn new_edge_during_service_waits_for_eoi_and_is_not_lost() {
    for irq in 0..8 {
        let mut pic = Pic::new();
        pic.set_irq_line(irq, true);
        pic.acknowledge().unwrap();
        pic.set_irq_line(irq, false);
        pic.set_irq_line(irq, true);
        assert!(!pic.interrupt_pending());
        pic.end_interrupt(None);
        assert!(pic.interrupt_pending());
        assert_eq!(pic.acknowledge().unwrap(), 0x08 + irq);
        pic.end_interrupt(None);
        assert!(!pic.interrupt_pending());
    }
}

#[test]
fn mask_preserves_requests_until_they_are_unmasked() {
    let mut pic = Pic::new();
    pic.imr = 0xFF;
    for irq in 0..8 {
        pic.set_irq_line(irq, true);
    }
    assert!(!pic.interrupt_pending());

    // Unmask one at a time, including lower-priority requests while higher
    // requests remain pending but masked.
    for irq in (0..8).rev() {
        pic.imr = !(1 << irq);
        assert!(pic.interrupt_pending());
        assert_eq!(pic.acknowledge().unwrap(), 0x08 + irq);
        pic.end_interrupt(None);
        assert!(!pic.interrupt_pending());
    }
    pic.imr = 0;
    assert!(!pic.interrupt_pending());
}

#[test]
fn in_service_priority_blocks_same_and_lower_but_allows_higher_irqs() {
    for active in 0..8 {
        for requested in 0..8 {
            let mut pic = Pic::new();
            pic.set_irq_line(active, true);
            pic.acknowledge().unwrap();
            pic.set_irq_line(requested, false);
            pic.set_irq_line(requested, true);
            assert_eq!(
                pic.interrupt_pending(),
                requested < active,
                "in service: IRQ{active}, requested: IRQ{requested}"
            );
            if requested >= active {
                pic.end_interrupt(Some(active));
                assert!(pic.interrupt_pending());
            }
            assert_eq!(pic.acknowledge().unwrap(), 0x08 + requested);
        }
    }
}

#[test]
fn nonspecific_eoi_unwinds_nested_service_in_priority_order() {
    let mut pic = Pic::new();
    pic.set_irq_line(6, true);
    assert_eq!(pic.acknowledge().unwrap(), 0x0E);
    pic.set_irq_line(1, true);
    assert_eq!(pic.acknowledge().unwrap(), 0x09);
    pic.set_irq_line(3, true);
    assert!(!pic.interrupt_pending());

    pic.end_interrupt(None);
    assert_eq!(pic.isr, 0x40, "IRQ6 remains in service after IRQ1's EOI");
    assert!(pic.interrupt_pending());
    assert_eq!(pic.acknowledge().unwrap(), 0x0B);
    pic.end_interrupt(None);
    assert_eq!(pic.isr, 0x40);
    pic.end_interrupt(None);
    assert_eq!(pic.isr, 0);
    assert!(!pic.interrupt_pending());
}

#[test]
fn specific_eoi_clears_only_the_named_in_service_irq() {
    let mut pic = Pic::new();
    pic.set_irq_line(6, true);
    pic.acknowledge().unwrap();
    pic.set_irq_line(1, true);
    pic.acknowledge().unwrap();
    pic.set_irq_line(3, true);

    pic.end_interrupt(Some(3)); // Pending, but not in service.
    assert_eq!(pic.isr, 0x42);
    assert!(!pic.interrupt_pending());
    pic.end_interrupt(Some(6));
    assert_eq!(pic.isr, 0x02);
    assert!(!pic.interrupt_pending());
    pic.end_interrupt(Some(1));
    assert!(pic.interrupt_pending());
    assert_eq!(
        pic.acknowledge().unwrap(),
        0x0B,
        "EOI must not erase a pending request"
    );
}

#[test]
fn masking_an_in_service_irq_does_not_release_its_priority_block() {
    let mut pic = Pic::new();
    pic.set_irq_line(1, true);
    pic.acknowledge().unwrap();
    pic.imr = 0x02;
    pic.set_irq_line(6, true);
    assert!(!pic.interrupt_pending());
    // Normal mask mode: EOI still clears the masked in-service IRQ.
    pic.end_interrupt(None);
    assert!(pic.interrupt_pending());
    assert_eq!(pic.acknowledge().unwrap(), 0x0E);
}
