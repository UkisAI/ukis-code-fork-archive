# IBM Plex Mono

The UkisAI website uses IBM Plex Mono for technical labels, navigation, and metadata. The Ukis Windows Terminal profile uses the same family for the CLI's fixed-width layout.

These four unmodified TrueType files come from IBM's [Plex Mono 2.5.0 release](https://github.com/IBM/plex/releases/tag/%40ibm%2Fplex-mono%402.5.0), `fonts/complete/ttf`. They are distributed under the accompanying [SIL Open Font License](license.txt).

Source archive: `ibm-plex-mono.zip`  
SHA-256: `6d23f01257663d8cc49a0d64c22ced630b79e0e2a0ac08a0da86e9a38bbc481c`

Run `powershell -File scripts/install-ukis-terminal.ps1` to install the fonts for the current Windows user and add the **Ukis** terminal profile. Then run `ukis window` from a project folder.

The font belongs to the terminal profile. Running ordinary `ukis` inside another terminal uses that terminal's font. On macOS or Linux, install these font files and select **IBM Plex Mono** in your terminal's preferences.

To remove the profile, delete `%LOCALAPPDATA%\Microsoft\Windows Terminal\Fragments\Ukis\ukis.json`. The installed font can be uninstalled through Windows Settings → Personalization → Fonts.
