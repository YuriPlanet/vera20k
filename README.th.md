<img src="docs/images/new-conscirpt-hero-image.png" alt="ภาพหน้าปก VERA20k" width="100%">

<p align="center" dir="ltr">
  <a href="README.sv.md" lang="sv">Svenska</a> · <a href="README.zh-CN.md" lang="zh-CN">简体中文</a> · <a href="README.de.md" lang="de">Deutsch</a> · <a href="README.ar.md" lang="ar" dir="rtl">العربية</a> · <a href="README.ru.md" lang="ru">Русский</a> · <strong>ไทย</strong> · <a href="README.tr.md" lang="tr">Türkçe</a> · <a href="README.md" lang="en">English</a>
  &nbsp;&nbsp;
  <a href="https://github.com/YuriPlanet/vera20k/actions/workflows/macos.yml?query=branch%3Amain" title="การทดสอบไลบรารีบน macOS ครั้งล่าสุด (สั่งรันด้วยตนเอง)"><img src="https://github.com/YuriPlanet/vera20k/actions/workflows/macos.yml/badge.svg?branch=main" alt="การทดสอบไลบรารีบน macOS" height="20" align="middle"></a>
  <a href="https://github.com/YuriPlanet/vera20k/actions/workflows/linux.yml?query=branch%3Amain" title="การทดสอบไลบรารีบน Linux ครั้งล่าสุด (สั่งรันด้วยตนเอง)"><img src="https://github.com/YuriPlanet/vera20k/actions/workflows/linux.yml/badge.svg?branch=main" alt="การทดสอบไลบรารีบน Linux" height="20" align="middle"></a>
  <a href="https://github.com/YuriPlanet/vera20k/actions/workflows/windows.yml?query=branch%3Amain" title="การทดสอบไลบรารีบน Windows ครั้งล่าสุด (สั่งรันด้วยตนเอง)"><img src="https://github.com/YuriPlanet/vera20k/actions/workflows/windows.yml/badge.svg?branch=main" alt="การทดสอบไลบรารีบน Windows" height="20" align="middle"></a>
  <a href="https://discord.gg/kmjRUn5m5F"><img src="https://img.shields.io/badge/Discord-Join-5865F2?style=flat&amp;logo=discord&amp;logoColor=white" alt="เข้าร่วม Discord" height="20" align="middle"></a>
</p>

# VERA20k

Red Alert 2: Yuri's Revenge — เขียนขึ้นใหม่ด้วย Rust สำหรับการต่อสู้แบบผู้เล่นหลายคนขนาดใหญ่

VERA20k คือการเขียนเอนจินเกมต้นฉบับ `gamemd.exe` ขึ้นใหม่ โดยใช้ไฟล์จากเกมต้นฉบับ
คุณจึงต้องมี Red Alert 2: Yuri's Revenge ของตัวเอง เกมนี้มีรวมอยู่ใน *Command & Conquer The Ultimate Collection*
บน [Steam](https://store.steampowered.com/bundle/39394/) และ
[EA](https://www.ea.com/games/command-and-conquer/command-and-conquer-the-ultimate-collection/buy/pc)

VERA20k สร้างโดยเกมเมอร์ เพื่อเกมเมอร์ และผู้เล่นคือผู้ตัดสินใจขั้นสุดท้ายว่าโครงการจะไปในทิศทางใด

<img src="docs/images/vera20k-screenshots.png" alt="หน้าตั้งค่าการต่อสู้และภาพขณะเล่น VERA20k" width="100%">

## เป้าหมายของโครงการ

1. คงรูปแบบการเล่น ภาพ และบรรยากาศของ Red Alert 2: Yuri's Revenge ต้นฉบับไว้
2. รองรับการต่อสู้ที่ใหญ่ขึ้น: สูงสุด **30 ผู้เล่น** และ **20,000 ยูนิต** บนแผนที่ขนาดใหญ่ขึ้น
3. เพิ่มฟีเจอร์ใหม่สำหรับเกม RTS
4. ไคลเอนต์ผู้เล่นหลายคนในตัว

## สถานะปัจจุบัน

**อยู่ในช่วงเริ่มต้นของการพัฒนา** สามารถเล่นการต่อสู้ภายในเครื่องบน Windows กับ AI พื้นฐานได้แล้ว
มีแผนที่จากเกมต้นฉบับและแผนที่สุ่ม เมนู การสร้างฐาน การเก็บทรัพยากร การต่อสู้ และการบันทึก/โหลดเกมแล้ว
แต่ยังมีอีกมากที่ต้องแก้ไขและทำให้เสร็จ

ยังไม่มีโหมดผู้เล่นหลายคน แคมเปญ และ AI แบบเกมต้นฉบับ เครื่องบิน การควบคุมจิตใจ สะพาน
รวมถึงอาวุธและเอฟเฟกต์หลายอย่างยังต้องพัฒนาต่อ เรายังไม่ได้สาธิตการต่อสู้ที่มีผู้เล่น 30 คนและยูนิต 20,000 ตัว

## บิลด์และรัน

ต้องมี Rust 1.88 หรือใหม่กว่า GPU ที่รองรับ Vulkan, DirectX 12 หรือ Metal และติดตั้งเกมไว้แล้ว
มีการเล่น VERA20k บน Windows, Linux และ macOS แล้ว

```sh
git clone https://github.com/YuriPlanet/vera20k.git
cd vera20k
cp config.toml.example config.toml
# ก่อนรัน ให้แก้ไข config.toml และตั้งค่า ra2_dir เป็นโฟลเดอร์เกมของคุณ:
cargo run --release --bin vera20k
```

ใช้ `--release` เพื่อเล่นเกม เพราะบิลด์แบบ debug ช้าเกินไป ดูวิธีตั้งค่าบนแต่ละแพลตฟอร์มและวิธีรันทดสอบได้ที่
[CONTRIBUTING.md](CONTRIBUTING.md#set-up)

## วิธีทำงานของเรา

โค้ดส่วนใหญ่เขียนโดยเอเจนต์ AI ที่ฉันคอยกำกับ เราใช้ Ghidra ศึกษาเอนจินต้นฉบับ
แล้วนำพฤติกรรมของมันมาเขียนใน Rust และตรวจสอบด้วย[เครื่องมือเปรียบเทียบ](tools/native_oracle.md)
และการทดลองเล่น กติกาการทำงานอยู่ใน [AGENTS.md](AGENTS.md)

## ร่วมพัฒนา

ยินดีรับความช่วยเหลือ คุณจะเขียนโค้ด ทดสอบเกม ปรับปรุงเอกสาร หรือเล่นเทียบกับเกมต้นฉบับ
แล้วบอกเราว่าตรงไหนรู้สึกผิดไปก็ได้ ไม่จำเป็นต้องมีประสบการณ์ด้านวิศวกรรมย้อนกลับเพื่อร่วมช่วย

อ่าน [CONTRIBUTING.md](CONTRIBUTING.md) ดู[งานที่เหมาะสำหรับผู้เริ่มต้น](https://github.com/YuriPlanet/vera20k/labels/good%20first%20issue)
หรือแวะมาทักทายใน [Discord](https://discord.gg/kmjRUn5m5F)
[ภาพรวมสถาปัตยกรรม](https://yuriplanet.github.io/vera20k/th/) อธิบายว่าส่วนต่าง ๆ ของเอนจินทำงานร่วมกันอย่างไร

## เครดิตและข้อกฎหมาย

ขอบคุณ OpenRA, XCC Mixer, วิกิ ModEnc, Project Perfect Mod, EA ที่เผยแพร่ซอร์สโค้ดของ
Command & Conquer และ Red Alert ภายใต้ GPL, World-Altering Editor, Final Alert, YRpp, Ares, Phobos
และอีกหลายโครงการ

เผยแพร่ภายใต้สัญญาอนุญาต [GPLv3](LICENSE-GPL) ที่เก็บโค้ดนี้ไม่มีไฟล์เกม
Command & Conquer และ Red Alert เป็นเครื่องหมายการค้าของ Electronic Arts Inc.
ภาพหน้าจอแสดงงานภาพจากเกมที่เป็นของ Electronic Arts
VERA20k ไม่มีความเกี่ยวข้องกับ Electronic Arts และไม่ได้รับการรับรองจาก Electronic Arts

แปลจาก [README.md](README.md) โปรดปรับปรุงให้ตรงกับฉบับภาษาอังกฤษเสมอ
