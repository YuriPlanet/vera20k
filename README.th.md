<img src="docs/images/new-conscirpt-hero-image.png" alt="ภาพหน้าปก VERA20K" width="100%">

<p align="center" dir="ltr">
  <a href="README.sv.md" lang="sv">Svenska</a> · <a href="README.zh-CN.md" lang="zh-CN">简体中文</a> · <a href="README.de.md" lang="de">Deutsch</a> · <a href="README.ar.md" lang="ar" dir="rtl">العربية</a> · <a href="README.es.md" lang="es">Español</a> · <a href="README.ja.md" lang="ja">日本語</a> · <a href="README.ru.md" lang="ru">Русский</a> · <strong>ไทย</strong> · <a href="README.tr.md" lang="tr">Türkçe</a> · <a href="README.md" lang="en">English</a>
</p>
<p align="center" dir="ltr">
  <a href="https://github.com/YuriPlanet/vera20k/actions/workflows/macos.yml?query=branch%3Amain" title="การทดสอบไลบรารีบน macOS ครั้งล่าสุด (สั่งรันด้วยตนเอง)"><img src="https://github.com/YuriPlanet/vera20k/actions/workflows/macos.yml/badge.svg?branch=main" alt="การทดสอบไลบรารีบน macOS" height="23" align="middle"></a>
  <a href="https://github.com/YuriPlanet/vera20k/actions/workflows/linux.yml?query=branch%3Amain" title="การทดสอบไลบรารีบน Linux ครั้งล่าสุด (สั่งรันด้วยตนเอง)"><img src="https://github.com/YuriPlanet/vera20k/actions/workflows/linux.yml/badge.svg?branch=main" alt="การทดสอบไลบรารีบน Linux" height="23" align="middle"></a>
  <a href="https://github.com/YuriPlanet/vera20k/actions/workflows/windows.yml?query=branch%3Amain" title="การทดสอบไลบรารีบน Windows ครั้งล่าสุด (สั่งรันด้วยตนเอง)"><img src="https://github.com/YuriPlanet/vera20k/actions/workflows/windows.yml/badge.svg?branch=main" alt="การทดสอบไลบรารีบน Windows" height="23" align="middle"></a>
  <a href="https://github.com/YuriPlanet/vera20k/actions/workflows/wasm.yml?query=branch%3Amain" title="การบิลด์ WebAssembly ครั้งล่าสุด (สั่งรันด้วยตนเอง)"><img src="https://github.com/YuriPlanet/vera20k/actions/workflows/wasm.yml/badge.svg?branch=main" alt="การบิลด์ WebAssembly" height="23" align="middle"></a>
  <a href="https://discord.gg/kmjRUn5m5F"><img src="https://img.shields.io/badge/Discord-Join-5865F2?style=flat&amp;logo=discord&amp;logoColor=white" alt="เข้าร่วม Discord" height="23" align="middle"></a>
  <img src="https://img.shields.io/badge/QQ-1097193563-1EBAFC?style=flat&amp;logo=qq&amp;logoColor=white" alt="กลุ่ม QQ" height="23" align="middle">
</p>

# VERA20K

VERA20K คือโครงการที่ขับเคลื่อนโดยชุมชนเพื่อเขียนเอนจินเกมต้นฉบับ `gamemd.exe` ขึ้นใหม่
โดยใช้ไฟล์จากเกมต้นฉบับ คุณจึงต้องมี Red Alert 2: Yuri's Revenge ของตัวเอง

VERA20K สร้างโดยเกมเมอร์ เพื่อเกมเมอร์ และเกมเมอร์คือผู้ตัดสินใจขั้นสุดท้ายว่าโครงการจะไปในทิศทางใด

<img src="docs/images/vera20k-screenshots.png" alt="หน้าตั้งค่าการต่อสู้และภาพขณะเล่น VERA20K" width="100%">

## เป้าหมายของโครงการ

1. คงรูปแบบการเล่น ภาพ และบรรยากาศของ Red Alert 2: Yuri's Revenge ต้นฉบับไว้
2. รองรับการต่อสู้ที่ใหญ่ขึ้น: สูงสุด **30 ผู้เล่น** และ **20,000 ยูนิต** บนแผนที่ขนาด 500×500
3. เพิ่มฟีเจอร์ RTS ที่เป็นที่รู้จัก รวมถึงฟีเจอร์ที่ไม่เคยมีมาก่อน
4. ไคลเอนต์ผู้เล่นหลายคนในตัว

ดูงานที่ให้ความสำคัญในตอนนี้และแผนในอนาคตได้ใน[แผนการพัฒนา](https://github.com/orgs/YuriPlanet/projects/1)

## ดาวน์โหลดและเล่น

ดาวน์โหลดตัวติดตั้งสำหรับ Windows, macOS หรือ Linux
ได้จาก[หน้ารุ่นที่เผยแพร่](https://github.com/YuriPlanet/vera20k/releases) ตัวติดตั้งจะค้นหา
Yuri's Revenge ของคุณ แล้วติดตั้ง VERA20K ไว้ในโฟลเดอร์ใหม่

หากต้องการเริ่มการต่อสู้ภายในเครื่องหรือแคมเปญของ YR แบบผู้เล่นคนเดียวผ่านไคลเอนต์ CnCNet
โปรดดู[การตั้งค่าไคลเอนต์แบบออฟไลน์](integrations/cncnet/README.md)

## ร่วมพัฒนา

**ไม่ต้องกลัวที่จะส่ง PR** ไม่จำเป็นต้องสมบูรณ์แบบ ที่เหลือเราช่วยคุณเอง

ยินดีรับความช่วยเหลือทุกด้าน แต่ด้านต่อไปนี้ต้องการคนช่วยมากที่สุด แต่ละด้านลิงก์ไปยังหน้าของด้านนั้นในแผนการพัฒนา ซึ่งบอกงานที่กำลังทำและขั้นตอนถัดไป:

- **[รูปแบบการเล่นและการต่อสู้](https://github.com/YuriPlanet/vera20k/issues/3830):** พฤติกรรมของยูนิต ความเสียหาย และความเสถียรของการต่อสู้ภายในเครื่อง
- **[ประสิทธิภาพ](https://github.com/YuriPlanet/vera20k/issues/3831):** อัตราเฟรมและการตอบสนองในการต่อสู้ขนาดใหญ่และบนฮาร์ดแวร์รุ่นเก่า
- **[โหมดผู้เล่นหลายคน](https://github.com/YuriPlanet/vera20k/issues/3832):** การเข้าร่วม เริ่ม และเล่นแมตช์ออนไลน์ต่อเนื่อง รวมถึงความเสถียรของการเล่นแบบร่วมมือ
- **[UI และเสียง](https://github.com/YuriPlanet/vera20k/issues/3833):** เมนู ข้อความ [การแปล](src/i18n/README.md) และเสียง
- **[แคมเปญ](https://github.com/YuriPlanet/vera20k/issues/3834):** เริ่มภารกิจได้แล้ว แต่การเล่นให้จบยังต้องพัฒนาต่อ: เป้าหมาย ทริกเกอร์ การเปลี่ยนฉากตามสคริปต์ การบันทึก/โหลด และการไปภารกิจถัดไป
- **[ความเข้ากันได้กับม็อด](https://github.com/YuriPlanet/vera20k/issues/3835):** ฟีเจอร์ของ Ares และ Phobos โดยไม่เปลี่ยนค่าเริ่มต้นของเกมต้นฉบับ
- **[การทดสอบ](https://github.com/YuriPlanet/vera20k/issues?q=is%3Aopen%20label%3Aparity):** เล่น VERA20K เทียบกับเกมต้นฉบับ แล้วรายงานส่วนที่เล่นต่างไป

ผู้ร่วมพัฒนาสามารถรับผิดชอบส่วนต่าง ๆ ของเอนจินได้ โดยมีอิสระอย่างมากในการเปลี่ยนแปลงที่ช่วยผลักดัน VERA20K ไปข้างหน้า

ผู้ดูแลโครงการจะช่วยในการรีวิว การผสานรวม และการเมิร์จ คุณจะได้รับเครดิตเต็มที่สำหรับผลงานของคุณ

การพูดคุยอยู่ใน [Discord](https://discord.gg/kmjRUn5m5F) และในกลุ่ม QQ (1097193563)

## สถาปัตยกรรม

เอนจินแบ่งเป็นสามส่วนหลัก:

- **การจำลอง:** ดูแลสถานะและตรรกะของเกม
- **การเรนเดอร์:** ไปป์ไลน์การเรนเดอร์ด้วย GPU ที่สร้างขึ้นใหม่ทั้งหมด ใช้ทรัพยากรเกมต้นฉบับ
  และสร้างขึ้นเพื่อคงรูปลักษณ์เดิมไว้
- **แอปพลิเคชัน:** จัดการอินพุต เมนู เสียง และการทำงานร่วมกับแพลตฟอร์ม พร้อมเชื่อมระบบต่าง ๆ เข้าด้วยกัน

การแยกส่วนนี้ช่วยให้ผลการจำลองเกมคงที่เมื่อเริ่มจากสถานะและอินพุตเดียวกัน

ลูกศรแสดงการไหลของข้อมูล:

![แอปพลิเคชันส่งคำสั่งไปยังการจำลองและรับสถานะของโลกกลับมา แล้วส่งข้อมูลการวาดไปยังการเรนเดอร์](docs/images/architecture.th.png)

## บิลด์และรัน

สิ่งที่ต้องมี:

- **Rust และ Git:** [Rust](https://rustup.rs/) 1.97 หรือใหม่กว่า ที่ติดตั้งผ่าน rustup และ [Git](https://git-scm.com/)
- **เครื่องมือบิลด์:** บน Windows ใช้ C++ build tools ของ Visual Studio ซึ่งตัวติดตั้ง Rust จะเสนอให้ติดตั้ง
  บน macOS รัน `xcode-select --install` บน Debian และ Ubuntu รัน `sudo apt install build-essential libasound2-dev libxkbcommon-x11-0 pkg-config`

```sh
git clone https://github.com/YuriPlanet/vera20k.git
cd vera20k
cp config.toml.example config.toml
# ก่อนรัน ให้แก้ไข config.toml และตั้งค่า ra2_dir เป็นโฟลเดอร์เกมของคุณ:
cargo run --release --bin vera20k
```

## วิธีทำงานของเรา

โค้ดส่วนใหญ่เขียนโดยเอเจนต์ AI ซึ่งใช้ Ghidra ศึกษาเอนจินต้นฉบับ
แล้วนำพฤติกรรมของมันมาเขียนใน Rust และตรวจสอบด้วย[เครื่องมือเปรียบเทียบ](tools/native_oracle.md)
และการทดลองเล่น เอเจนต์ทำตาม [AGENTS.md](AGENTS.md)

## การปรับแต่ง

ระบบเลือกความจุแผนที่อัตโนมัติโดยไม่ต้องใช้แฟล็กบิลด์: ผลรวมความกว้างและความสูงของ `Size` สูงสุดคือ 1024

โดยค่าเริ่มต้น VERA20K จะเล่นได้เหมือนเกมต้นฉบับ ฟีเจอร์ใหม่อื่น ๆ รวมถึงส่วนขยายจากม็อดอย่าง Ares
ต้องปิดไว้จนกว่าจะเปิดด้วยวิธีใดวิธีหนึ่งต่อไปนี้:

- **ตัวเลือกของผู้เล่นใน VERA** อยู่ใน **Options** และบันทึกในส่วน `[VERA]` ของ `VERA.INI`
- **ตัวเลือกตอนบิลด์** เช่น `--features ares` สำหรับ[ส่วนขยาย Ares](docs/ares.md)ที่รองรับ
- **คีย์ INI** ที่ม็อดหรือแผนที่กำหนด

การตั้งค่าเสียง กราฟิก และการควบคุมจะบันทึกไว้ใน `VERA.INI` กฎการเล่นมาจากการตั้งค่าล็อบบี้
ม็อด หรือแผนที่

[คู่มือการทำม็อด](docs/modding.md)อธิบายวิธีสร้างม็อดของคุณเอง

## สนับสนุนโครงการ

<a href="https://paypal.me/vera20k"><img src="https://img.shields.io/badge/Donate_via_PayPal-FFC439?style=flat&amp;logo=paypal&amp;logoColor=003087" alt="สนับสนุน VERA20K ผ่าน PayPal" height="23"></a>

เงินบริจาคทั้งหมดที่ได้รับจะจัดสรรให้
ผู้มีส่วนร่วมตาม[บัญชีบันทึกผลงาน TIBTRE](https://github.com/YuriPlanet/tibtre-ledger)

ผู้มีส่วนร่วมรับเงินได้ผ่าน:

<p><img src="https://img.shields.io/badge/PayPal-003087?style=flat&amp;logo=paypal&amp;logoColor=white" alt="PayPal" height="23"> <img src="https://img.shields.io/badge/China_UnionPay-E21836?style=flat&amp;logo=data:image/svg%2Bxml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHZpZXdCb3g9Ii02IC02IDI2OCAxNzIiPjxwYXRoIGQ9Ik01MC4wNDE2MzEsMC4wMDA2Njc1NjMzMDUgTDExMy44MjAyMTEsMC4wMDA2Njc1NjMzMDUgQzEyMi43MjMxMjcsMC4wMDA2Njc1NjMzMDUgMTI4LjI2MDQzOCw3LjI1NzMxMzQ1IDEyNi4xODM1NTgsMTYuMTg5NzY1NyBMOTYuNDkwMDcxNSwxNDMuNjc1NDE2IEM5NC4zOTQ1MzY1LDE1Mi41NzY3NzcgODUuNDgwNzM5LDE1OS44Mzk2NDEgNzYuNTcxNjA1LDE1OS44Mzk2NDEgTDEyLjc5OTI0MzMsMTU5LjgzOTY0MSBDMy45MDg3NjQsMTU5LjgzOTY0MSAtMS42NDA5ODQ0NCwxNTIuNTc2Nzc3IDAuNDM1ODk2MjA4LDE0My42NzU0MTYgTDMwLjE0MTgxODYsMTYuMTg5NzY1NyBDMzIuMjE4Njk5Myw3LjI1NzMxMzQ1IDQxLjEyNzgzMzMsMC4wMDA2Njc1NjMzMDUgNTAuMDQxNjMxLDAuMDAwNjY3NTYzMzA1IiBmaWxsPSIjRTIxODM2IiBzdHJva2U9IiNmZmYiIHN0cm9rZS13aWR0aD0iOCIgc3Ryb2tlLWxpbmVqb2luPSJyb3VuZCIvPjxwYXRoIGQ9Ik0xMDguNTEzMjgzLDAuMDAwNjY3NTYzMzA1IEwxODEuODU3MDE4LDAuMDAwNjY3NTYzMzA1IEMxOTAuNzU4Mzc4LDAuMDAwNjY3NTYzMzA1IDE4Ni43NDQ1MjcsNy4yNTczMTM0NSAxODQuNjUwNTQ2LDE2LjE4OTc2NTcgTDE1NC45NjE3MjQsMTQzLjY3NTQxNiBDMTUyLjg3ODYyNSwxNTIuNTc2Nzc3IDE1My41Mjg0MjcsMTU5LjgzOTY0MSAxNDQuNjA4NDExLDE1OS44Mzk2NDEgTDcxLjI2NDY3NzUsMTU5LjgzOTY0MSBDNjIuMzQ0NjYxNSwxNTkuODM5NjQxIDU2LjgyNDQ0OTUsMTUyLjU3Njc3NyA1OC45MTk5ODUsMTQzLjY3NTQxNiBMODguNjA3MjUyNSwxNi4xODk3NjU3IEM5MC43MTUyMjQsNy4yNTczMTM0NSA5OS42MDU3MDM1LDAuMDAwNjY3NTYzMzA1IDEwOC41MTMyODMsMC4wMDA2Njc1NjMzMDUiIGZpbGw9IiMwMDQ0N0MiIHN0cm9rZT0iI2ZmZiIgc3Ryb2tlLXdpZHRoPSI4IiBzdHJva2UtbGluZWpvaW49InJvdW5kIi8%2BPHBhdGggZD0iTTE3OC45NDc4MywwLjAwMDY2NzU2MzMwNSBMMjQyLjcyNjQwOSwwLjAwMDY2NzU2MzMwNSBDMjUxLjY0MTc2MiwwLjAwMDY2NzU2MzMwNSAyNTcuMTc5MDc0LDcuMjU3MzEzNDUgMjU1LjA4NTA5MywxNi4xODk3NjU3IEwyMjUuMzk2MjcxLDE0My42NzU0MTYgQzIyMy4zMDA3MzYsMTUyLjU3Njc3NyAyMTQuMzgwNzE5LDE1OS44Mzk2NDEgMjA1LjQ2NjkyMiwxNTkuODM5NjQxIEwxNDEuNzE3ODc5LDE1OS44Mzk2NDEgQzEzMi43OTc4NjMsMTU5LjgzOTY0MSAxMjcuMjY1MjE1LDE1Mi41NzY3NzcgMTI5LjM1NDUzMiwxNDMuNjc1NDE2IEwxNTkuMDQ4MDE3LDE2LjE4OTc2NTcgQzE2MS4xMjQ4OTksNy4yNTczMTM0NSAxNzAuMDI3ODE0LDAuMDAwNjY3NTYzMzA1IDE3OC45NDc4MywwLjAwMDY2NzU2MzMwNSIgZmlsbD0iIzAwN0I4NCIgc3Ryb2tlPSIjZmZmIiBzdHJva2Utd2lkdGg9IjgiIHN0cm9rZS1saW5lam9pbj0icm91bmQiLz48L3N2Zz4%3D" alt="China UnionPay" height="23"> <img src="https://img.shields.io/badge/Alipay-1677FF?style=flat&amp;logo=alipay&amp;logoColor=white" alt="Alipay" height="23"> <img src="https://img.shields.io/badge/Vipps-FF5B24?style=flat&amp;logo=data:image/svg%2Bxml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHZpZXdCb3g9IjQzLjUgMjEuNSAyNyAyNyI%2BPHBhdGggZD0iTTU3LjMsNDAuNmMzLjcsMCw1LjgtMS44LDcuOC00LjRjMS4xLTEuNCwyLjUtMS43LDMuNS0wLjlzMS4xLDIuMywwLDMuN2MtMi45LDMuOC02LjYsNi4xLTExLjMsNi4xIGMtNS4xLDAtOS42LTIuOC0xMi43LTcuN2MtMC45LTEuMy0wLjctMi43LDAuMy0zLjRzMi41LTAuNCwzLjQsMUM1MC41LDM4LjMsNTMuNSw0MC42LDU3LjMsNDAuNnogTTY0LjIsMjguM2MwLDEuOC0xLjQsMy0zLDMgcy0zLTEuMi0zLTNzMS40LTMsMy0zQzYyLjgsMjUuMyw2NC4yLDI2LjYsNjQuMiwyOC4zeiIgZmlsbD0iI2ZmZiIvPjwvc3ZnPg%3D%3D" alt="Vipps" height="23"> <img src="https://img.shields.io/badge/SEPA-2D4B9A?style=flat&amp;logo=sepa&amp;logoColor=white" alt="SEPA" height="23"> <img src="https://img.shields.io/badge/Bitcoin-F7931A?style=flat&amp;logo=bitcoin&amp;logoColor=white" alt="Bitcoin" height="23"></p>

## เครดิตและข้อกฎหมาย

ขอบคุณ OpenRA, XCC Mixer, วิกิ ModEnc, Project Perfect Mod, EA ที่เผยแพร่ซอร์สโค้ดของ
Command & Conquer และ Red Alert ภายใต้ GPL, World-Altering Editor, Final Alert, YRpp, Ares, Phobos
และอีกหลายโครงการ

ลิขสิทธิ์ © 2026 Halvor Nikolai Rusten และผู้ร่วมพัฒนา VERA20K
เผยแพร่ภายใต้สัญญาอนุญาต [GPLv3](LICENSE-GPL) พร้อม[ข้อกำหนดเพิ่มเติม](LEGAL.md#additional-terms) ที่เก็บโค้ดนี้ไม่มีไฟล์เกม
Command & Conquer และ Red Alert เป็นเครื่องหมายการค้าของ Electronic Arts Inc.
ภาพหน้าจอและไอคอนแอป (ดาวแดงของ Red Alert 2) แสดงงานภาพจากเกมที่เป็นของ Electronic Arts
VERA20K ไม่มีความเกี่ยวข้องกับ Electronic Arts และไม่ได้รับการรับรองจาก Electronic Arts

This product includes software developed by the [Ares Contributors](LICENSES/Ares.txt).

**โปรดอ่าน[ประกาศทางกฎหมาย](LEGAL.md)ก่อนแชร์ ขาย หรือ fork VERA20K**

## เอกสาร

- [รุ่นทดลอง WebAssembly](docs/wasm.md): บิลด์สำหรับผู้เล่นคนเดียวแบบเลือกใช้ได้ ซึ่งทำงานในเบราว์เซอร์
- [คู่มือการทำม็อด](docs/modding.md): สิ่งที่ม็อดทำได้และทำไม่ได้ และ VERA20K แตกต่างจากเกมต้นฉบับอย่างไร
- [โปรไฟล์ INI](docs/ini-roots.md): การเลือกไฟล์กฎ ไฟล์ art และไฟล์ AI
- [Ares](docs/ares.md), [Phobos](docs/phobos.md) และ [NPatch](docs/npatch.md): การรองรับส่วนขยายและข้อจำกัด
- [ภาพรวมของเอนจิน](docs/index.md): โครงสร้างโค้ดและการทำงานร่วมกันของส่วนต่าง ๆ ในเอนจิน
- [เครื่องมือสำหรับนักพัฒนา](tools/README.md): การบิลด์ การทดสอบ และการตรวจสอบทรัพยากรเกม
- [การทดสอบแผนที่](tools/map_observation.md): ฉากทดสอบที่ใช้สคริปต์ ภาพหน้าจอ และการสังเกตการณ์แมตช์
- [การเปรียบเทียบกับต้นฉบับ](tools/native_oracle.md): การเปรียบเทียบพฤติกรรมกับเกมต้นฉบับ
- [ขั้นตอนการทำงานกับ Ghidra](docs/ghidra-workflow.md): การศึกษาเอนจินต้นฉบับ

แปลจาก [README.md](README.md) โปรดปรับปรุงให้ตรงกับฉบับภาษาอังกฤษเสมอ
