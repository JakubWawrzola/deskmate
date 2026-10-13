# PawnIO modules

Signed modules for the [PawnIO](https://pawnio.eu) driver, used by the optional
CPU temperature helper (`src/cpu_temperature.rs`). They are embedded in x64
builds only and loaded unmodified.

| File | Purpose | SHA-256 |
|---|---|---|
| `IntelMSR.bin` | Intel: `IA32_PACKAGE_THERM_STATUS`, `IA32_TEMPERATURE_TARGET` | `d6ed85d65ab17a22f813ef98207d6d537155ee2ded5976a21cb48413c9b92e5f` |
| `AMDFamily17.bin` | AMD Zen (family 17h-1Ah): SMN `0x59800`, `THM_TCON_CUR_TMP` | `dae74615761b78bdf064dfb3e136252ddcc6fc727d88f14738d0e5800d427a91` |

Source: [namazso/PawnIO.Modules](https://github.com/namazso/PawnIO.Modules),
release [0.2.11](https://github.com/namazso/PawnIO.Modules/releases/tag/0.2.11)
(`release_0_2_11.zip`, SHA-256
`43608cb89bc84247fef1368a139013f7d043e17db6d6c8dfc9b46bf0905a81f4`).
License: GNU LGPL 2.1, see `COPYING`; the module sources (`IntelMSR.p`,
`AMDFamily17.p`) are in that repository.

Deskmate talks to PawnIO only through PawnIOLib (installed with PawnIO) and the
driver's device IO control interface, as PawnIO's license exception allows.
