# Translating VERA20k

VERA20k keeps its own text in this folder, with one file per language.
`vera20k.en.ini` is the English text, and the other files are translations of it.

You only need a text editor and the game. You don't need to program or build anything.

## Translate a language

### 1. Get the file

- **A new language:** download the
  [English file](https://github.com/YuriPlanet/vera20k/blob/main/src/i18n/vera20k.en.ini)
  and rename it to `vera20k.<code>.ini` with your language's code: `vi` for Vietnamese,
  `fr` for French, `pt-BR` for Brazilian Portuguese.
- **A language that already exists:** download the newest copy of its file from this folder.

### 2. Translate it

Open the file in a plain text editor, such as Notepad, and save it as UTF-8.
Put your language's own name after `Name=`, then change the text after each `=`.
For example, in German:

```ini
[Language]
Name=Deutsch

[Strings]
VERA:SaveSetup=Setup speichern
VERA:CatchingUp[one]=Holt auf - {count} Frame zurück
VERA:CatchingUp[other]=Holt auf - {count} Frames zurück
```

| You see | What to do |
| --- | --- |
| The text before the `=` | Leave it exactly as it is. |
| `{count}`, `{name}` and others in `{ }` | Keep them. The game fills them in. You can move them to suit the sentence. |
| `\n` | This starts a new line. Keep it. |
| A line starting with `;` | A note for you. The game doesn't show it. A `;` in the middle of your text is shown as normal. |
| `[one]`, `[few]`, `[other]` after a label | The text for different numbers. Vietnamese, Chinese and Thai only need `[other]`, so delete the others. German needs `[one]` and `[other]`. Russian needs `[one]`, `[few]`, `[many]` and `[other]`. Step 4 tells you what yours needs. |

Keep text about as long as the English. Buttons have little room, and lines whose
label ends in `Help` must fit on one line.

VERA20k already includes fonts for most scripts, and right-to-left text works, so you
don't need to do anything for those.

### 3. Try it in the game

Put your file in the VERA20k settings folder, the one with `VERA.INI` in it:

| System | Folder |
| --- | --- |
| Windows | The folder you installed the game in, next to `vera20k.exe` |
| macOS | `~/Library/Application Support/VERA20k/` |
| Linux | `~/.local/share/VERA20k/` |

Start the game and pick your language under **Options**, **Language**. Restart the game
each time you change the file.

You can test a half-finished file. Lines you haven't translated yet show in English.

### 4. See what's left

Each time the game starts, it writes a list of what your file is missing to its log,
`logs/ra2.log`. Look at the end of that file. If the last start added no list, your
file is complete.

The `logs` folder is inside the settings folder.

### 5. Send it

Send your file to us on [Discord](https://discord.gg/kmjRUn5m5F) or in the QQ group
(1097193563), or open a pull request that adds it to this folder.

A new language goes into the game once every line is translated. When it does, delete
your copy from the settings folder. A copy there replaces the one in the game, so any
new lines would show in English.

## Movie subtitles and names on the sidebar

Two more files can sit next to your language file, with your language's code in their
names. Both are optional, and the game reads them from the same folder.

- **`vera20k.<code>.subtitles.txt`** holds the subtitles for the game's movies. Copy the
  game's own `subtitlemd.txt`, translate the text in it, and save it as UTF-16 with a
  byte order mark, as the game's file is saved.
- **`vera20k.<code>.cameos`** holds the names drawn on the sidebar's pictures, which are
  part of each picture. Draw your names on copies of the game's 60x48 cameos, then
  make this file with `python -m tools.language_assets cameos`; the top of
  [its script](../../tools/language_assets.py) explains how. The file keeps only the
  difference from the game's own name rows, so it holds none of the game's art.

Vietnamese has both. They and the letters in `vera20k.vi.letters` come from the language
pack credited at the top of [its language file](vera20k.vi.ini).

## Where the languages are after installing

The game has English built in. An installed game keeps every other language in a
`languages` folder, as the same files that are in this folder, along with the fonts for
other writing systems:

| System | Folder |
| --- | --- |
| Windows | `languages`, next to `vera20k.exe` |
| macOS | `Contents/Resources/languages` inside `VERA20K.app` |
| Linux | `usr/share/vera20k/languages` inside the AppImage |

On Windows, delete a language's files from that folder to take it out of the game. The
next update puts it back. A font can go too, but it isn't tied to one language: without
it, any text only that font draws shows as empty boxes, even a player's name in an English
game. The macOS app and the Linux AppImage are sealed when they are made, so for those,
delete the files from the `languages` folder that
[`languages.sh`](../../.github/release/languages.sh) makes before packaging.

## Keep a language up to date

When the English text changes, the person who changes it also updates every language
in the same change. No language falls behind, but those updates are often quick
translations that a native speaker could improve.

To review them, look at
[what changed in this folder](https://github.com/YuriPlanet/vera20k/commits/main/src/i18n/).
If something reads badly, start from the newest copy of your language's file, fix it and
send it the same way as above.

## If you change the English text

For developers. Whenever you change `vera20k.en.ini`, change every other language file
in the same pull request:

- **A new line:** add it to every file. The tests fail if a language is missing it.
- **New wording for a line:** change that line in every file. No test catches this, so
  check it yourself.

`vera20k --check-language <file>` lists what a file is missing, and the exact file format
is explained at the top of [`mod.rs`](mod.rs).

## Other text

- **The original game's text**, such as unit names and missions, comes from the player's
  game files. A language file can replace it using the game's own labels, such as
  `NAME:MTNK=` or `GUI:Ok=`, as the Vietnamese file does. Ask on Discord for the list of
  labels, and keep any `%s` or `%d` in that text.
- **The README and project page** are `README.<code>.md` and `docs/<code>/index.md`.
- **The website** (vera20k.org) isn't in this repository. Ask on Discord.
