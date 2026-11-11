# Installing, repairing and uninstalling

Each platform has one installer file. Run it for the Setup wizard. On Linux, make the installer executable first, then run it.

The installer checks its own checksum before it changes anything. If the file is damaged it stops with [QAD2103](messages-reference.md#QAD2103); download it again.

## The Setup wizard

Move through the pages with **Next >** and **< Back**. **Cancel** leaves Setup after asking.

| Page | What to do |
|---|---|
| **Welcome** | Select **Next >**. |
| **Licence Agreement** | Read the licence and select **I accept the licence terms**. **Next >** is disabled until you do. |
| **Install For** | Choose **Just me** or **All users**. See the next section. |
| **Install Location** | Keep the folder shown, type the full path of another, or select **Browse...**. |
| **Shortcuts and File Types** | Choose the menu entries, **A shortcut on the desktop**, and whether the program's file types open with it. All three are selected to begin with. |
| **Ready to Install** | Check the summary of your choices, then select **Install**. |
| **Installing** | Wait while the files are copied. |
| **Setup Complete** | Read the result. Leave **Start QS/400 Access for Desktop** selected to start the program, then select **Finish**. |

If the program is running when you select **Install**, Setup asks to close it. **Close It** closes it and disconnects its open sessions; **Cancel** goes back to the wizard.

Anything you should know about, such as a shortcut that could not be made, is listed on the last page under **Please note**. If the install fails, the last page is titled **Setup Did Not Complete**, and whatever Setup had made is removed again.

## Just me or All users

| | Just me | All users |
|---|---|---|
| Windows folder | `%LOCALAPPDATA%\Programs\Quartz Systems\Access` | `%ProgramFiles%\Quartz Systems\Access` |
| Windows rights | None needed | Windows asks for administrator rights when the install starts |
| Linux folder | `~/.local/opt/quartz-access` | `/opt/quartz-access` |
| Linux commands | Links in `~/.local/bin` | Links in `/usr/local/bin` |
| Linux menu entries, file types and icons | Under `~/.local/share` | Under `/usr/local/share` |
| Linux rights | None needed | Run the installer as root |

Without the rights **All users** needs, Setup reports [QAD2104](messages-reference.md#QAD2104) and installs nothing.

On Windows the Start menu folder is **Quartz Systems**. On Linux, Setup warns, without failing, when `~/.local/bin` is not on your `PATH` or when a desktop tool it uses to register file types is missing.

## File types

When the file types option is selected, these open with the program:

| Extension | Kind of file |
|---|---|
| `.qsd` | Display session |
| `.qsp` | Printer session |
| `.qkm` | Keyboard map |
| `.qsm` | Macro |
| `.qdx` | Data Transfer request |

## What Setup records

Every file, folder, shortcut and file type Setup makes is listed in `install-manifest.toml` in the install folder. Repair and uninstall work from that list. Setup also keeps a copy of itself in the install folder as the uninstaller: `uninstall.exe` on Windows, `quartz-access-uninstall` on Linux, where it is also linked as a command.

## Upgrading

To upgrade, run the newer installer; there is no need to uninstall first. The **Welcome** page names the release found and the wizard starts with that install's choices. Setup removes the old program files, installs the new ones and reports [QAD2102](messages-reference.md#QAD2102). Your settings, system configurations and session files are kept.

## Repairing

Repair writes the program files, shortcuts and file types again, with the choices the install was made with. It does not change your settings, system configurations or session files.

- Run the uninstaller with `--repair`, then select **Repair**.
- On Windows you can also choose to modify the program in the system's list of installed apps.

Setup reports [QAD2106](messages-reference.md#QAD2106) when it is done.

## Uninstalling

1. On Windows, uninstall the program from the system's list of installed apps, or run `uninstall.exe` in the install folder. On Linux, run `quartz-access-uninstall`.
2. To remove your settings and system configurations as well, select **Also remove my settings**.
3. Select **Uninstall**.

Setup removes exactly what the install manifest lists and reports [QAD2105](messages-reference.md#QAD2105). Your Sessions folder is never removed. An install for all users needs administrator rights (Windows asks) or root (Linux) to repair or uninstall.

## Silent mode

Run the installer with `--silent` to install without the wizard.

| Option | Meaning |
|---|---|
| `--silent` | No windows. For an install, `--scope` is required. |
| `--scope=user` or `--scope=machine` | Just me, or All users. `machine` needs administrator rights or root. |
| `--dir=PATH` | The install folder, as a full path. |
| `--no-start-menu` | No menu entries. |
| `--no-desktop-shortcut` | No desktop shortcut. |
| `--no-associations` | Do not register the file types. |
| `--close-running` | Close the program if it is running. Without this, a silent run stops with exit code 6. |
| `--log=FILE` | Where to write the log. Otherwise it is `qsa-install.log` in the temporary folder. |

Run the uninstaller with `--uninstall` or `--repair`. Both take `--silent` and `--close-running`; `--uninstall` also takes `--remove-settings`.

| Exit code | Meaning |
|---|---|
| 0 | Done |
| 1 | Arguments not understood |
| 2 | Installer damaged ([QAD2103](messages-reference.md#QAD2103)) |
| 3 | Administrator or root rights needed ([QAD2104](messages-reference.md#QAD2104)) |
| 4 | Cancelled |
| 5 | A file or registry key could not be written or removed; see the log |
| 6 | QS/400 Access for Desktop is running |
| 7 | No install found to uninstall or repair |

## The Windows warning for an unsigned installer

The installer is not code-signed yet. The first time you run it from a download, Windows shows **Windows protected your PC**. Select **More info**, check that the file name shown is the installer you downloaded, then select **Run anyway**. The installed program is not signed either.

## See also

- [Getting started](getting-started.md)
- [Session Manager and session files](session-manager.md)
- [Running a request with qsaccess run](qsaccess-run.md)
- [Messages reference](messages-reference.md)
