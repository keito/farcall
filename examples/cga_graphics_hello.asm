; Build: nasm -f bin -Werror examples/cga_graphics_hello.asm -o examples/cga_graphics_hello.com
; Run:   cargo run -- --com examples/cga_graphics_hello.com
; Disk: mcopy -o -i examples/CGAHELLO.IMA examples/cga_graphics_hello.com ::CGAHELLO.COM
; Expected: HELLO WORLD centered, with HEL / LO W / ORLD in colors 1/2/3.
; Cycle all three RGB CGA palettes, each at normal and bright intensity.
; Each state lasts 18 BIOS ticks (about 0.99 seconds on an IBM PC).
; Press any key to restore 80-column text mode and exit to DOS.
;
; Requires BIOS INT 10h mode 04h, INT 16h keyboard, INT 1Ah clock,
; CGA graphics rendering, and ports 3D8h/3D9h.
; The letters are our own bitmap: no BIOS graphics character service needed.
; CGA layout reference: IBM Color/Graphics Monitor Adapter, graphics modes:
; https://minuszerodegrees.net/oa/OA%20-%20IBM%20Color%20Graphics%20Monitor%20Adapter%20(CGA).pdf

bits 16
cpu 8086
org 100h

    mov ax, 0004h       ; BIOS: 320x200, four colors.
    int 10h

    push cs
    pop ds
    cld
    mov ax, 0B800h
    mov es, ax          ; ES:DI addresses CGA video memory.
    xor di, di
    xor ax, ax
    mov cx, 2000h      ; Clear all 16 KiB, including both scanline banks.
    rep stosw

    ; Four pixels per byte, most significant pair first.
    ; offset(x,y) = (y & 1)*2000h + (y/2)*80 + x/4
    ; Our 88x8 bitmap begins at x=116, y=96.
    mov di, (96 / 2) * 80 + (116 / 4)
    mov si, message_bitmap
    mov bp, 8          ; Eight scanlines in the bitmap.

draw_row:
    mov dx, 11         ; Eleven eight-pixel chunks per scanline.
draw_chunk:
    lodsb              ; Read eight monochrome pixels, leftmost in bit 7.
    ; DX counts down from 11: HEL=1, LO W=2, ORLD=3.
    ; The space stays black, so the middle colored letters read LOW.
    mov ah, 1
    cmp dx, 8
    ja color_ready
    inc ah
    cmp dx, 4
    ja color_ready
    inc ah
color_ready:
    xor bx, bx
    mov cx, 8
expand_pixel:
    shl bx, 1
    shl bx, 1          ; Make room for the next two-bit CGA pixel.
    shl al, 1          ; Move the next bitmap bit into Carry.
    jnc black_pixel
    or bl, ah          ; A set bitmap bit takes this letter's color index.
black_pixel:
    loop expand_pixel

    ; BX now holds eight pixels. Store the LEFT four first (BH).
    ; STOSW would reverse these bytes on the little-endian 8088.
    mov al, bh
    stosb
    mov al, bl
    stosb
    dec dx
    jnz draw_chunk

    sub di, 22         ; Return to the start of this scanline.
    xor di, 2000h      ; Alternate between even and odd scanline banks.
    test di, 2000h
    jnz next_row       ; Even -> odd: same offset within the bank.
    add di, 80         ; Odd -> even: advance one row within the bank.
next_row:
    dec bp
    jnz draw_row

    call apply_palette
    sti                ; BIOS clock ticks need timer interrupts enabled.
    xor ah, ah
    int 1Ah            ; CX:DX = ticks since midnight.
    mov [last_tick], dx

wait_for_tick:
    mov ah, 01h        ; Check without blocking, so animation can continue.
    int 16h
    jnz exit_program

    xor ah, ah
    int 1Ah
    test al, al        ; BIOS reports midnight rollover in AL.
    jnz restart_timer  ; Restart the short interval if the daily clock reset.
    mov ax, dx
    sub ax, [last_tick] ; Wrapping subtraction handles the low word wrapping.
    cmp ax, 18
    jb wait_for_tick
    mov [last_tick], dx

    inc byte [palette_index]
    cmp byte [palette_index], 6
    jb change_palette
    mov byte [palette_index], 0
change_palette:
    call apply_palette
    jmp short wait_for_tick

restart_timer:
    mov [last_tick], dx
    jmp short wait_for_tick

exit_program:
    xor ah, ah         ; Consume the key that ended the animation.
    int 16h
    mov ax, 0003h      ; BIOS: restore 80x25 text mode.
    int 10h
    mov ax, 4C00h      ; DOS: exit successfully.
    int 21h

apply_palette:
    xor bx, bx
    mov bl, [palette_index]
    shl bx, 1          ; Each state contains two port values.
    mov dx, 03D8h
    mov al, [palette_states + bx]
    out dx, al
    inc dx             ; 3D9h: palette selector, intensity, black background.
    mov al, [palette_states + bx + 1]
    out dx, al
    ret

palette_index: db 0
last_tick: dw 0

;       3D8h  3D9h    Colors for pixel indices 1, 2, 3 on an RGB monitor
; 3D8h bit 2 selects the third palette (monochrome on composite output).
; 3D9h bit 5 selects the normal palette's blue component; bit 4 is intensity.
; Neither register change clears or redraws the bitmap.
palette_states:
    db 0Ah, 00h        ; green, red, brown
    db 0Ah, 10h        ; light green, light red, yellow
    db 0Ah, 20h        ; cyan, magenta, light gray
    db 0Ah, 30h        ; light cyan, light magenta, white
    db 0Eh, 00h        ; cyan, red, light gray (third palette)
    db 0Eh, 10h        ; light cyan, light red, white

; Original 8x8 letter shapes, stored by scanline, not by character.
; Each byte describes one letter's row; 1=letter color, 0=black.
;       H    E    L    L    O   space  W    O    R    L    D
message_bitmap:
    db 66h, 7Eh, 60h, 60h, 3Ch, 00h, 66h, 3Ch, 7Ch, 60h, 78h
    db 66h, 60h, 60h, 60h, 66h, 00h, 66h, 66h, 66h, 60h, 6Ch
    db 66h, 60h, 60h, 60h, 66h, 00h, 66h, 66h, 66h, 60h, 66h
    db 7Eh, 7Ch, 60h, 60h, 66h, 00h, 66h, 66h, 7Ch, 60h, 66h
    db 66h, 60h, 60h, 60h, 66h, 00h, 7Eh, 66h, 6Ch, 60h, 66h
    db 66h, 60h, 60h, 60h, 66h, 00h, 7Eh, 66h, 66h, 60h, 6Ch
    db 66h, 7Eh, 7Eh, 7Eh, 3Ch, 00h, 66h, 3Ch, 66h, 7Eh, 78h
    db 00h, 00h, 00h, 00h, 00h, 00h, 00h, 00h, 00h, 00h, 00h
