<img src="docs/images/new-conscirpt-hero-image.png" alt="الصورة الرئيسية لمشروع VERA20k" width="100%">

<p align="center" dir="ltr">
  <a href="README.sv.md" lang="sv">Svenska</a> · <a href="README.zh-CN.md" lang="zh-CN">简体中文</a> · <a href="README.de.md" lang="de">Deutsch</a> · <strong lang="ar" dir="rtl">العربية</strong> · <a href="README.ru.md" lang="ru">Русский</a> · <a href="README.th.md" lang="th">ไทย</a> · <a href="README.tr.md" lang="tr">Türkçe</a> · <a href="README.md" lang="en">English</a>
  &nbsp;&nbsp;
  <a href="https://github.com/YuriPlanet/vera20k/actions/workflows/macos.yml?query=branch%3Amain" title="آخر تشغيل لاختبارات المكتبة على macOS (تُشغّل يدويًا)"><img src="https://github.com/YuriPlanet/vera20k/actions/workflows/macos.yml/badge.svg?branch=main" alt="اختبارات المكتبة على macOS" height="20" align="middle"></a>
  <a href="https://github.com/YuriPlanet/vera20k/actions/workflows/linux.yml?query=branch%3Amain" title="آخر تشغيل لاختبارات المكتبة على Linux (تُشغّل يدويًا)"><img src="https://github.com/YuriPlanet/vera20k/actions/workflows/linux.yml/badge.svg?branch=main" alt="اختبارات المكتبة على Linux" height="20" align="middle"></a>
  <a href="https://github.com/YuriPlanet/vera20k/actions/workflows/windows.yml?query=branch%3Amain" title="آخر تشغيل لاختبارات المكتبة على Windows (تُشغّل يدويًا)"><img src="https://github.com/YuriPlanet/vera20k/actions/workflows/windows.yml/badge.svg?branch=main" alt="اختبارات المكتبة على Windows" height="20" align="middle"></a>
  <a href="https://discord.gg/kmjRUn5m5F"><img src="https://img.shields.io/badge/Discord-Join-5865F2?style=flat&amp;logo=discord&amp;logoColor=white" alt="انضم إلى Discord" height="20" align="middle"></a>
</p>

<div dir="rtl">

# VERA20k

لعبة Red Alert 2: Yuri's Revenge — أُعيد بناؤها بلغة Rust لمعارك كبيرة متعددة اللاعبين.

يعيد VERA20k كتابة المحرك الأصلي، `gamemd.exe`. يستخدم ملفات اللعبة الأصلية، لذا ستحتاج إلى
نسختك الخاصة من Red Alert 2: Yuri's Revenge. تتوفر اللعبة ضمن *Command & Conquer The Ultimate Collection*
على [Steam](https://store.steampowered.com/bundle/39394/) و
[EA](https://www.ea.com/games/command-and-conquer/command-and-conquer-the-ultimate-collection/buy/pc).

VERA20k من صنع اللاعبين ومن أجلهم، وللاعبين الكلمة الأخيرة في تحديد مساره.

<img src="docs/images/vera20k-screenshots.png" alt="شاشة إعداد المناوشات ومشهد من اللعب في VERA20k" width="100%">

## أهداف المشروع

1. الحفاظ على أسلوب اللعب والمظهر والأجواء في لعبة Red Alert 2: Yuri's Revenge الأصلية.
2. دعم معارك أكبر: حتى **30 لاعبًا** و**20,000 وحدة** على خرائط أكبر.
3. إضافة ميزات جديدة لألعاب الاستراتيجية في الوقت الحقيقي (RTS).
4. عميل مدمج للعب متعدد اللاعبين

## الوضع الحالي

**في مرحلة مبكرة من التطوير.** يمكن لعب المناوشات محليًا على Windows ضد ذكاء اصطناعي بسيط.
خرائط اللعبة الأصلية والخرائط العشوائية والقوائم وبناء القواعد وجمع الموارد والقتال وحفظ اللعبة
وتحميلها موجودة، لكن ما زال هناك الكثير لإصلاحه وإكماله.

ما زال اللعب الجماعي والحملات والذكاء الاصطناعي الأصلي غير متوفر. تحتاج الطائرات والتحكم بالعقول
والجسور وعدة أسلحة وتأثيرات إلى مزيد من العمل. لم نعرض بعد معارك تضم 30 لاعبًا و20,000 وحدة.

## البناء والتشغيل

تحتاج إلى Rust 1.88 أو أحدث، وبطاقة رسوميات تدعم Vulkan أو DirectX 12 أو Metal، ونسخة مثبتة
من اللعبة. وقد استُخدم VERA20k للعب على Windows وLinux وmacOS.

<div dir="ltr">

```sh
git clone https://github.com/YuriPlanet/vera20k.git
cd vera20k
cp config.toml.example config.toml
# عدّل config.toml واضبط ra2_dir على مجلد اللعبة قبل التشغيل:
cargo run --release --bin vera20k
```

</div>

استخدم `--release` للعب؛ فنسخ debug بطيئة جدًا. راجع
[CONTRIBUTING.md](CONTRIBUTING.md#set-up) لمعرفة إعدادات المنصات المختلفة وطريقة تشغيل الاختبارات.

## كيف نعمل

يكتب معظم الكود وكلاء برمجة بالذكاء الاصطناعي أتولى توجيههم. نستخدم Ghidra لدراسة المحرك الأصلي،
ثم ننقل سلوكه إلى Rust ونتحقق منه باستخدام [أدوات المقارنة](tools/native_oracle.md) واختبارات اللعب.
قواعد العمل موجودة في [AGENTS.md](AGENTS.md).

## المساهمة

نرحب بالمساعدة. يمكنك كتابة الكود، أو اختبار اللعبة، أو تحسين التوثيق، أو لعبها جنبًا إلى جنب مع
الأصل وإخبارنا بما يبدو غير صحيح. لا تحتاج إلى خبرة في الهندسة العكسية للمساعدة.

اقرأ [CONTRIBUTING.md](CONTRIBUTING.md)، أو تصفّح
[المهام المناسبة للبداية](https://github.com/YuriPlanet/vera20k/labels/good%20first%20issue)، أو ألقِ
التحية على [Discord](https://discord.gg/kmjRUn5m5F).
تشرح [نظرة عامة على البنية](https://yuriplanet.github.io/vera20k/ar/) كيف تتكامل أجزاء المحرك.

## شكر ومعلومات قانونية

نشكر OpenRA وXCC Mixer وويكي ModEnc وProject Perfect Mod، وشركة EA على نشر الكود المصدري
للعبتي Command & Conquer وRed Alert تحت رخصة GPL، وكذلك World-Altering Editor وFinal Alert
وYRpp وAres وPhobos وكثيرين غيرهم.

المشروع مرخّص بموجب [GPLv3](LICENSE-GPL). لا يحتوي هذا المستودع على أي ملفات للعبة. تُعد
Command & Conquer وRed Alert علامتين تجاريتين لشركة Electronic Arts Inc.، وتعرض لقطات الشاشة
رسومات من اللعبة تملكها Electronic Arts. لا يتبع VERA20k شركة Electronic Arts ولا يحظى بتأييدها.

ترجمة لـ [README.md](README.md)؛ يُرجى تحديثها مع النسخة الإنجليزية.

</div>
