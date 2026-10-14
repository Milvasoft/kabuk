# OledSaver

A lightweight (≈50 KB) Windows screen saver for OLED displays, written from scratch.

- Two themes; in both, the background is fully black and no element stays in a fixed place:
  - **Milvasoft** (default): built around Milva's circuit-board eye. The eye opens somewhere on the screen with the geometry from the logo, blinks now and then, and its pupil follows the flowing signals. PCB traces come out of the pads on its legs; independent traces also draw themselves on the screen. The traces advance on a 45° grid without crossing each other and end in vias or ring pads. Yellow-orange signals flow along the traces and light up the pad they reach. The "milva" text and the clock sit next to the eye. Every 40-65 seconds the eye closes and opens somewhere else, and the traces live for a while and then fade out; so nothing stays on the screen permanently. Colors are taken from the logo: yellow `#FBC419`, orange `#F38424`, gray `#C0BFBE`.
  - **Takımyıldız**: connected dots whose color changes slowly, and a large clock.
- Covers every monitor separately, with a different pattern flowing on each monitor.
- Closes when the mouse moves (more than 10 px), on a click, on a wheel turn or on any key press.
- No extra dependencies: runs on .NET Framework 4.8, which ships with Windows 10/11.

## Installation

1. Extract the folder somewhere.
2. Double-click `install.cmd`.

The script copies `OledSaver.scr` to `%LOCALAPPDATA%\OledSaver`, selects it as the screen saver and sets the wait time to **5 minutes**. No admin rights are needed, and there is no need to sign out and back in.

For a different time: `powershell -ExecutionPolicy Bypass -File install.ps1 -TimeoutMinutes 3`

To uninstall, `uninstall.cmd`.

## When does it kick in?

Windows itself manages the idle time and the "don't start while a video is playing" rule; the app does not run in the background for this.

- It starts if the keyboard/mouse is not used for 5 min.
- Apps that play video (browsers, VLC, MPC, Netflix, etc.) send Windows a "keep the display on" request; while this request is active, the screen saver does not start.
- If it doesn't start when it should, run `powercfg /requests` in a terminal opened as administrator; the app listed under `DISPLAY` is blocking it.

## Settings

The **Settings** button in Windows Screen Saver Settings, or `OledSaver.scr /c`:

| Setting | Description |
| --- | --- |
| Theme | Milvasoft or Takımyıldız |
| Show time and date | Roaming time/date |
| Brightness | Maximum brightness of everything drawn |
| Density | Number of dots per 1920×1080 area |
| Speed | Flow speed |

Settings are stored under `HKCU\Software\OledSaver`.

Extra tip for OLED: in the power settings, keep the "turn off the display" time longer than the screen saver's (for example 15-20 min). That way the animation starts first, and if you stay away for a long time the panel turns off completely.

## Building from source

```
cd src
dotnet build -c Release
```

Output: `src/bin/Release/net48/OledSaver.scr`

The command-line arguments are the Windows standard: `/s` fullscreen, `/p <HWND>` preview, `/c` or no argument for settings.
