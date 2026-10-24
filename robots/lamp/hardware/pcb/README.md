# PCB

Lamp boards: schematic PDF, Altium source, Gerbers.

| Board | Folder | Schematic | Altium project |
|---|---|---|---|
| Pi hat | `pi-hat/` | `SCH_Pi_Hat_LampVer1.pdf` | `altium/Prj_Lamb/` |
| Head power | `head-pwr/` | `SCH_Head_PWR_Lamp.pdf` | `altium/PCB_Head_Lamp/` |
| Input button | `input-button/` | `SCH_Input_Button.pdf` | `altium/Lamp_PWR_Board/` |

## Each board folder

- `SCH_*.pdf` - schematic, read first
- `altium/` - source, open in Altium
- `gerber/` - fab files, zip and send

## Order a board

- zip the board's `gerber/`
- upload to JLCPCB / EasyEDA
- guide: `gerber/How-to-order-PCB.txt`
- `FlyingProbeTesting.json` - fab probe test, keep it

## Notes

- revision 2026-10-24
- Pi hat: 2nd sheet in `altium/Prj_Lamb/Schematic2/`
- Input button: project still named `Lamp_PWR_Board`
- Altium + PDF via Git LFS -> `git lfs pull` after clone
- new revision: replace files, update date above
