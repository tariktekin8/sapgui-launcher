# SAP GUI Launcher

A desktop app that opens SAP GUI already logged in to a chosen system. It runs on macOS (Apple silicon), Windows and Linux. Linux is not tested yet.

## Scope
- Import direct application-server systems and their folders from the SAP Logon landscape file. Systems that are already in the list keep everything except their folder, and a folder the user chose stays.
- Add, edit, duplicate and delete systems by hand. A name is required; the system number takes at most two digits. After a new system is saved, the form for its first user opens.
- **Duplicate** copies a system with all its users, under a new name that can be changed before saving. The copy is not a favorite and not in Recent.
- Add, rename, move and delete folders, also inside other folders. Imported folders work the same way. Deleting a folder moves its content one folder up; no system is deleted.
- Move systems and folders by drag and drop (mouse or pen) or in their forms. A drop on **All systems** moves to the top level.
- Save several users for each system: client, user, language and password.
- Environment label and notes.
- Fixed folders that cannot be edited or deleted: **Favorites**, **Recent** (the last five used systems that are not favorites) and **All systems**. A favorite system has a star in the list. Open and closed folders are not remembered after a restart.
- Search by system name, SID, client, user and folder.
- Launch a user with the launch icon in its row or with the tray menu. The tray menu lists favorites, recent systems and all systems that have users, in their folders. Each entry shows client, user and language. If a launch from the tray menu fails, the window opens and shows the error.
- A copied password is marked as secret for clipboard managers and cleared after 30 seconds or when the app quits. Closing the window hides passwords that are shown.
- Copy buttons, password generator and connection test (a TCP connection to port `32<sysnr>`).
- A dialog asks before a system, user or folder is deleted.
- Settings: theme (system, light or dark) and the SAP GUI file, chosen with the file picker of the OS: `sapshcut.exe` (Windows), the SAPGUI app (macOS) or `sapgui` (Linux). Any other file is refused.
- Closing the window keeps the app in the tray or menu bar.
- Update from GitHub Releases.

## Technology
- Tauri 2, with a Rust core and a React + TypeScript UI (Vite). The code is in `app/`.
- `std::process::Command` to launch SAP GUI, without a shell.

## Launch
| OS | Command |
|---|---|
| Windows | `sapshcut.exe -system=<SID> -client=<client> -user=<user> -pw=<password> -language=<lang>` (tested) |
| macOS / Linux | `SAPGUI "conn=/H/<host>/S/32<sysnr>&clnt=<client>&user=<user>&lang=<lang>&pass=<password>"` (tested on macOS with SAP GUI for Java 8.10) |

The app builds these commands from the saved system fields. The user never types a command or a connection string. On macOS and Linux a value with `&` is refused, because `&` separates the parameters of the connection string.

The app looks for SAP GUI in these folders, and in the "SAPGUI*" folders below them. It takes the last one by name. A file chosen under Settings is used instead.
| OS | Folder | Executable |
|---|---|---|
| Windows | `C:\Program Files\SAP\FrontEnd`, then `C:\Program Files (x86)\SAP\FrontEnd` | `sapshcut.exe` |
| macOS | `/Applications/SAP Clients` | `SAPGUI <version>.app/Contents/MacOS/SAPGUI` |
| Linux | `/opt/SAPClients` | `bin/sapgui` (not verified) |

Landscape file that **Import** reads:
| OS | Path |
|---|---|
| Windows | `%APPDATA%\SAP\Common\SAPUILandscape.xml` |
| macOS | `~/Library/Preferences/SAP/SAPGUILandscape.xml` |
| Linux | `~/.SAPGUI/SAPGUILandscape.xml` (not verified) |

## Data
- Each system has: name, SID, host, system number, environment, notes, folder, whether the user chose the folder, favorite flag, last use and a list of users.
- The file also holds all folders, imported or added by the user, and the path of the SAP GUI executable.
- The theme and the sidebar width are kept in the webview's local storage.
- Each user has: client, user, language, password.
- All fields, including the password, are stored unencrypted in `systems.json`, in the `sapguilauncher` folder of the OS app-config folder. On macOS and Linux only the user can read the file (`0600`).
- The password is also visible on the SAP GUI command line while it starts or runs.
- Never log the password.

## Distribution
- Distribution is through GitHub Releases only, with no store and no paid signing.
- **macOS:** an ad-hoc signed `.dmg` for Apple silicon.
  - On first run, the user clicks System Settings → Privacy & Security → **Open Anyway**.
  - Or the user runs `xattr -dr com.apple.quarantine "/Applications/<App>.app"`.
- **Windows:** an unsigned `.msi` and `-setup.exe`. On first run, the user clicks SmartScreen → **More info → Run anyway**.
- **Linux:** an AppImage, a `.deb` and an `.rpm`.
- Each installer and update file has a SHA-256 checksum.
- The app identifier is `sapguilauncher`. Changing it moves the data folder, so users would start with an empty list.

## Updates and versions
- The version lives only in `app/package.json`; `tauri.conf.json` reads it from there.
- At startup the app reads `latest.json` from the latest **published** GitHub release. Drafts are not seen.
- If a newer version exists, the sidebar shows it with **Update now**. After the click the app downloads it, checks its signature, installs it and restarts. A `.deb` or `.rpm` install asks for the administrator password.
- If the update fails, the app says so and links to the release page for a manual download.
- The workflow writes `latest.json` once, after all builds, with `.github/latest-json.sh`. It fails if a signature is missing.
- Update files are signed. The private key is encrypted with a password. The workflow reads both from the GitHub secrets `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. Both are also kept outside this repository; the public key is in `tauri.conf.json`. If the key or its password is lost, installed apps can no longer update themselves.
- To release:
  1. `cd app && npm version <x.y.z> --no-git-tag-version`
  2. Commit, then `git tag v<x.y.z> && git push origin main v<x.y.z>`.
  3. When the workflow finishes, publish the draft release on GitHub. Users are told about it from then on. The workflow never rebuilds a published version.

## TODO
- Test a password that contains `=` in the macOS conn string.
- Test that a copied password is cleared when the app quits.
- Test the conn string, the install path and the landscape path on Linux.
- Test the update from one published version to the next, also for `.deb` and `.rpm`.
