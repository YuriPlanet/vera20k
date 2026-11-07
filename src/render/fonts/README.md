# VERA's interface fonts

The vector fonts VERA draws its interface text with (`src/render/text_engine/`).
All are Noto fonts under the SIL Open Font License 1.1, taken unmodified:
`OFL.txt` covers the Noto Sans families and `OFL-CJK.txt` Noto Sans CJK.
The licence lets them ship inside the program; it must travel with them.

| File | Source |
| --- | --- |
| `NotoSans-VF.ttf` | notofonts.github.io `fonts/NotoSans/googlefonts/variable-ttf/NotoSans[wdth,wght].ttf` |
| `NotoSans<Script>-VF.ttf` | notofonts.github.io `fonts/NotoSans<Script>/googlefonts/variable-ttf/NotoSans<Script>[wdth,wght].ttf` |
| `NotoSansCJKsc-Regular.otf` | github.com/notofonts/noto-cjk `Sans/OTF/SimplifiedChinese/NotoSansCJKsc-Regular.otf` |

`<Script>` is ArabicUI, Armenian, Bengali, Devanagari, Ethiopic, Georgian,
Gujarati, Gurmukhi, Hebrew, Kannada, Khmer, Lao, Malayalam, Myanmar, Oriya,
Sinhala, Tamil, Telugu or Thai. Noto Sans covers Latin, Greek and Cyrillic;
Noto Sans CJK covers Chinese (Simplified and Traditional forms), Japanese and
Korean.

SHA-256 of each file as committed:

```
bfb7bb691513f12e734dc346c03a03f784912432d7e3fa8e56efcf906fe86b3d  NotoSans-VF.ttf
ab32083dafd79e2070a0e85944fc97bbe31d5352bd9c507e745a20ba18224ab8  NotoSansArabicUI-VF.ttf
c7b9235855077980d22636d2780c86c29420f9aa0e6d4058879698d44d0aa4e6  NotoSansArmenian-VF.ttf
dcd42978094e584a849c84a51450eeac40c8826057d566ea6d4b9627a403a05a  NotoSansBengali-VF.ttf
2c76254f6fc379fddfce0a7e84fb5385bb135d3e399294f6eeb6680d0365b74b  NotoSansCJKsc-Regular.otf
14ec4af41f27482216d1c2229f417ff9b1425e1babb014e57d1d40d03229853e  NotoSansDevanagari-VF.ttf
0dbccc00b22d180ebd6a4bd8a733918a29e709fa6798023adc3e6cd40da65077  NotoSansEthiopic-VF.ttf
f96abb48684395318c3d0fc525dd1695d2fa6459289533f3e6623ac14383fff4  NotoSansGeorgian-VF.ttf
f40c49e50b101dd884380c88d78649db819c2f0695e8080cc0d662abe33e4fc1  NotoSansGujarati-VF.ttf
1e6f728fa620e566f842d81e220265813faa12771214765d289c98e035adc5f2  NotoSansGurmukhi-VF.ttf
7ef36a2c3593758cdb622e1bdef4f84523e92fbc3ccc667438dd80ff54c2de88  NotoSansHebrew-VF.ttf
cca4f3b3a8cb12fb261f1b43baf5d2f7f59d90fe123d41f0065ed3a183997ec9  NotoSansKannada-VF.ttf
03b9f8efdfb5fdc9b23d2ba4e357ebc9e868968dce33739a8b248dae8fb1add0  NotoSansKhmer-VF.ttf
9608b94603a82d09a8038946f9775242f99e3b3459b7f1e4d5b335b578cd7ab3  NotoSansLao-VF.ttf
312e0e7c3cc15fa09eb42a8f749eeb246b593ed420e3c81aafe8d910c3a6fb56  NotoSansMalayalam-VF.ttf
7abbbfbe2514105d7ce94937aee3feb2ba89b73a256c8b77b5866bd9b83e32ec  NotoSansMyanmar-VF.ttf
910342275d619081b896b4ed25fd9ad6ff320c25591b810de1fcc4beb153ba82  NotoSansOriya-VF.ttf
10fe24516125b6b668fc7819911073188ac7c72263eb50a5c1e072c0ea44e958  NotoSansSinhala-VF.ttf
aa3a9b321f4b0bb2c40203ffbde9af89713227866e0e13f76e5b9eeea727cf88  NotoSansTamil-VF.ttf
3f6d917852921b1abfa9c6d3616e7d413e27b8e0f431adce9676b9089c1992d3  NotoSansTelugu-VF.ttf
db2bc1c62fae2df814edff7705ddb560d47d742274ed9947b7ddf1a6cb1c17b6  NotoSansThai-VF.ttf
```

How each script is sized against GAME.FNT is in `text_engine/fonts.rs`.
