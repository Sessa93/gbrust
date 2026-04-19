# gbrust

A Game Boy Color and Game Boy Advance emulator written in Rust.

## Features

- **Dual-console support** — plays both GBC (`.gb`/`.gbc`) and GBA (`.gba`) ROMs, auto-detected by file extension
- **Full CPU emulation**
  - SM83 (GBC): all 256 base opcodes + 256 CB-prefix opcodes
  - ARM7TDMI (GBA): ARM and THUMB instruction sets
- **Graphics** — scanline-based PPU for both consoles; GBA modes 0–5 with text/affine backgrounds and sprites
- **Audio** — square, wave, and noise channels (GBC); DirectSound FIFO A/B (GBA); real-time output via cpal
- **Memory** — MBC1/MBC3/MBC5 mappers, VRAM banking, OAM DMA, HDMA, GBA DMA (4 channels)
- **Save system** — battery-backed SRAM/Flash/EEPROM auto-save + 5 save-state slots (bincode serialization)
- **GUI** — eframe/egui window with menu bar, ROM file picker, aspect-ratio-correct rendering
- **Input** — keyboard mapping (Z/X = A/B, Arrows = D-Pad, Enter = Start, Backspace = Select, A/S = L/R)

## Building

Requires Rust 1.85+ (edition 2024).

```sh
cargo build --release
```

## Running

```sh
cargo run --release
```

Use **File → Open ROM** to load a `.gb`, `.gbc`, or `.gba` file.

## Controls

| Key        | Button |
| ---------- | ------ |
| Z          | A      |
| X          | B      |
| Enter      | Start  |
| Backspace  | Select |
| Arrow keys | D-Pad  |
| A          | L      |
| S          | R      |

## Save & Load

- SRAM is auto-saved alongside the ROM file every ~1 second of gameplay
- **Save State / Load State** via the menu bar (slots 1–5)

## Testing

```sh
cargo test
```

76 unit tests covering CPU instructions, timers, input, DMA, cartridges, PPU, APU, interrupts, and serialization round-trips.

## Project Structure

```
src/
├── cpu/          SM83 (GBC) and ARM7TDMI (GBA) CPUs
├── ppu/          Pixel processing units
├── apu/          Audio processing units
├── memory/       Memory bus wiring (GBC / GBA)
├── cartridge/    ROM loading, MBC mappers, backup detection
├── emulator/     Per-console frame loop
├── gui.rs        eframe/egui frontend + cpal audio
├── timer.rs      Hardware timers
├── input.rs      Joypad / key input
├── dma.rs        GBA DMA controller
├── interrupts.rs Interrupt flag constants
└── save.rs       SRAM and state-save persistence
```

## License

See [LICENSE](LICENSE).
