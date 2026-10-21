<img src="docs/images/new-conscirpt-hero-image.png" alt="VERA20K kapak görseli" width="100%">

<p align="center" dir="ltr">
  <a href="README.sv.md" lang="sv">Svenska</a> · <a href="README.zh-CN.md" lang="zh-CN">简体中文</a> · <a href="README.de.md" lang="de">Deutsch</a> · <a href="README.ar.md" lang="ar" dir="rtl">العربية</a> · <a href="README.es.md" lang="es">Español</a> · <a href="README.ru.md" lang="ru">Русский</a> · <a href="README.th.md" lang="th">ไทย</a> · <strong>Türkçe</strong> · <a href="README.md" lang="en">English</a>
</p>
<p align="center" dir="ltr">
  <a href="https://github.com/YuriPlanet/vera20k/actions/workflows/macos.yml?query=branch%3Amain" title="Son macOS kütüphane testi çalıştırması (elle başlatılır)"><img src="https://github.com/YuriPlanet/vera20k/actions/workflows/macos.yml/badge.svg?branch=main" alt="macOS kütüphane testleri" height="23" align="middle"></a>
  <a href="https://github.com/YuriPlanet/vera20k/actions/workflows/linux.yml?query=branch%3Amain" title="Son Linux kütüphane testi çalıştırması (elle başlatılır)"><img src="https://github.com/YuriPlanet/vera20k/actions/workflows/linux.yml/badge.svg?branch=main" alt="Linux kütüphane testleri" height="23" align="middle"></a>
  <a href="https://github.com/YuriPlanet/vera20k/actions/workflows/windows.yml?query=branch%3Amain" title="Son Windows kütüphane testi çalıştırması (elle başlatılır)"><img src="https://github.com/YuriPlanet/vera20k/actions/workflows/windows.yml/badge.svg?branch=main" alt="Windows kütüphane testleri" height="23" align="middle"></a>
  <a href="https://discord.gg/kmjRUn5m5F"><img src="https://img.shields.io/badge/Discord-Join-5865F2?style=flat&amp;logo=discord&amp;logoColor=white" alt="Discord'a katılın" height="23" align="middle"></a>
  <img src="https://img.shields.io/badge/QQ_grubu-1097193563-1EBAFC?style=flat&amp;logo=qq&amp;logoColor=white" alt="QQ grubu" height="23" align="middle">
</p>

# VERA20K

VERA20K, özgün oyun motoru `gamemd.exe`'nin topluluk öncülüğünde geliştirilen yeniden
yazımıdır. Orijinal oyun dosyalarını kullandığı için kendi Red Alert 2: Yuri's Revenge
kopyanıza ihtiyacınız var.

VERA20K oyuncular tarafından, oyuncular için yapılıyor ve projenin nereye gideceğine son sözü
oyuncular söylüyor.

<img src="docs/images/vera20k-screenshots.png" alt="VERA20K çatışma ayarları ekranı ve oyun içi görünüm" width="100%">

## Projenin hedefleri

1. Orijinal Red Alert 2: Yuri's Revenge'in oynanışını, görsellerini ve atmosferini korumak.
2. 500×500 haritalarda **30 oyuncuya** ve **20.000 birime** kadar daha büyük savaşları desteklemek.
3. Bilinen RTS özelliklerini ve daha önce hiç görülmemiş bazı özellikleri eklemek.
4. Entegre çok oyunculu istemci

## Mevcut durum

**Aktif geliştirme aşamasında.**

- **Çatışma:** Windows, Linux ve macOS'ta oynanabiliyor. Orijinal ve rastgele oluşturulan
  haritalar, üs kurma, kaynak toplama, çatışma ve kaydetme/yükleme mevcut.
- **Senaryolar:** Görevler başlatılabiliyor; destek henüz tamamlanmış değil.
- **Yapay zekâ:** Orijinal yapay zekânın bazı bölümleri üs kurma ve betiklerle yönetilen saldırıları yürütüyor.
- **Devam eden çalışmalar:** Oynanışın aslına uygunluğu, görseller ve performans.
- **Çok oyunculu mod:** Ağ üzerinden oyun henüz mevcut değil.
- **Ölçek:** 30 oyunculu, 20.000 birimli savaşlar henüz gösterilmiş değil.

## Mimari

Motorun üç ana bölümü vardır:

- **Simülasyon:** Oyun durumunu ve mantığını yönetir.
- **Görüntüleme:** Orijinal oyun kaynaklarını kullanan, orijinal görünümü korumak için
  oluşturulmuş tamamen yeni bir GPU görüntüleme hattı.
- **Uygulama:** Girdi, menüler, ses ve platform entegrasyonunu yönetir, sistemleri birbirine bağlar.

Bu ayrım deterministik oynanışı destekler.

Oklar veri akışını gösterir:

![Uygulama simülasyona komut gönderir ve dünya durumunu alır; görüntülemeye çizim verilerini gönderir.](docs/images/architecture.tr.png)

## Sistem gereksinimleri

- **Oyun:** Red Alert 2: Yuri's Revenge 1.001.
- **İşletim sistemi:** Windows 10+, Linux (glibc 2.35+, ör. Ubuntu 22.04 veya Steam Deck) ya da
  Apple Silicon üzerinde macOS 11+.
- **Grafik:** Vulkan, DirectX 12 ya da Metal.

## Derleme ve çalıştırma

Gerekenler:

- **Rust ve Git:** rustup ile kurulmuş [Rust](https://rustup.rs/) 1.97 veya üzeri ve
  [Git](https://git-scm.com/).
- **Derleme araçları:** Windows'ta, Rust kurulum programının kurmayı önerdiği Visual Studio C++
  derleme araçları; macOS'te `xcode-select --install`; Debian ve Ubuntu'da
  `sudo apt install build-essential libasound2-dev pkg-config`.

VERA20K, Windows, Linux ve macOS üzerinde oynandı.

```sh
git clone https://github.com/YuriPlanet/vera20k.git
cd vera20k
cp config.toml.example config.toml
# Çalıştırmadan önce config.toml dosyasını düzenleyip ra2_dir değerini oyun klasörünüz olarak ayarlayın:
cargo run --release --bin vera20k
```

Derleme `target/` içinde birkaç GB yer kaplar.

## Nasıl çalışıyoruz

Kodun büyük bölümünü yapay zekâ kodlama ajanları yazıyor. Ajanlar özgün motoru Ghidra ile
inceliyor, ardından davranışını Rust'a aktarıyor ve [karşılaştırma araçları](tools/native_oracle.md)
ile oyun testleri kullanarak kontrol ediyor. Ajanlar [AGENTS.md](AGENTS.md) dosyasını izler.

## Özelleştirme

VERA20K varsayılan olarak orijinal oyun gibi oynanır. Ares gibi modlardan gelen eklentiler dahil
her yeni özellik, şu yollardan biriyle açılana kadar kapalı kalmalıdır:

- **Bir derleme seçeneği**: örneğin desteklenen [Ares eklentileri](docs/ares.md) için `--features ares`.
- **Bir oyun içi lobi seçeneği**: her maç için seçilir.
- **Bir INI anahtarı**: bir mod ya da harita tarafından ayarlanır.

Tek oyunculu YR çatışmalarını veya senaryolarını bir CnCNet istemcisinden başlatmak için
[çevrimdışı istemci kurulumuna](integrations/cncnet/README.md) bakın.

## Katkıda bulunma

Yardımlarınızı bekliyoruz. Kod yazabilir, motoru yeniden düzenleyebilir, oyunu test edebilir,
fikirlerinizi paylaşabilir veya orijinal oyunla yan yana oynayıp neyin yanlış hissettirdiğini
bize anlatabilirsiniz. Bir PR açın, gerisini biz hallederiz.
Tartışmalar [Discord](https://discord.gg/kmjRUn5m5F) üzerinden ve QQ grubunda (1097193563) yürür.

## Destek

<a href="https://paypal.me/vera20k"><img src="https://img.shields.io/badge/Donate_via_PayPal-FFC439?style=flat&amp;logo=paypal&amp;logoColor=003087" alt="VERA20K'yi PayPal üzerinden destekleyin" height="23"></a>

Alınan tüm bağışlar,
[TIBTRE katkı defterine](https://github.com/YuriPlanet/tibtre-ledger) göre katkıda
bulunanlara dağıtılır.

Katkıda bulunanlara ödeme yöntemleri:

<p><img src="https://img.shields.io/badge/PayPal-003087?style=flat&amp;logo=paypal&amp;logoColor=white" alt="PayPal" height="23"> <img src="https://img.shields.io/badge/China_UnionPay-E21836?style=flat&amp;logo=data:image/svg%2Bxml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHZpZXdCb3g9Ii02IC02IDI2OCAxNzIiPjxwYXRoIGQ9Ik01MC4wNDE2MzEsMC4wMDA2Njc1NjMzMDUgTDExMy44MjAyMTEsMC4wMDA2Njc1NjMzMDUgQzEyMi43MjMxMjcsMC4wMDA2Njc1NjMzMDUgMTI4LjI2MDQzOCw3LjI1NzMxMzQ1IDEyNi4xODM1NTgsMTYuMTg5NzY1NyBMOTYuNDkwMDcxNSwxNDMuNjc1NDE2IEM5NC4zOTQ1MzY1LDE1Mi41NzY3NzcgODUuNDgwNzM5LDE1OS44Mzk2NDEgNzYuNTcxNjA1LDE1OS44Mzk2NDEgTDEyLjc5OTI0MzMsMTU5LjgzOTY0MSBDMy45MDg3NjQsMTU5LjgzOTY0MSAtMS42NDA5ODQ0NCwxNTIuNTc2Nzc3IDAuNDM1ODk2MjA4LDE0My42NzU0MTYgTDMwLjE0MTgxODYsMTYuMTg5NzY1NyBDMzIuMjE4Njk5Myw3LjI1NzMxMzQ1IDQxLjEyNzgzMzMsMC4wMDA2Njc1NjMzMDUgNTAuMDQxNjMxLDAuMDAwNjY3NTYzMzA1IiBmaWxsPSIjRTIxODM2IiBzdHJva2U9IiNmZmYiIHN0cm9rZS13aWR0aD0iOCIgc3Ryb2tlLWxpbmVqb2luPSJyb3VuZCIvPjxwYXRoIGQ9Ik0xMDguNTEzMjgzLDAuMDAwNjY3NTYzMzA1IEwxODEuODU3MDE4LDAuMDAwNjY3NTYzMzA1IEMxOTAuNzU4Mzc4LDAuMDAwNjY3NTYzMzA1IDE4Ni43NDQ1MjcsNy4yNTczMTM0NSAxODQuNjUwNTQ2LDE2LjE4OTc2NTcgTDE1NC45NjE3MjQsMTQzLjY3NTQxNiBDMTUyLjg3ODYyNSwxNTIuNTc2Nzc3IDE1My41Mjg0MjcsMTU5LjgzOTY0MSAxNDQuNjA4NDExLDE1OS44Mzk2NDEgTDcxLjI2NDY3NzUsMTU5LjgzOTY0MSBDNjIuMzQ0NjYxNSwxNTkuODM5NjQxIDU2LjgyNDQ0OTUsMTUyLjU3Njc3NyA1OC45MTk5ODUsMTQzLjY3NTQxNiBMODguNjA3MjUyNSwxNi4xODk3NjU3IEM5MC43MTUyMjQsNy4yNTczMTM0NSA5OS42MDU3MDM1LDAuMDAwNjY3NTYzMzA1IDEwOC41MTMyODMsMC4wMDA2Njc1NjMzMDUiIGZpbGw9IiMwMDQ0N0MiIHN0cm9rZT0iI2ZmZiIgc3Ryb2tlLXdpZHRoPSI4IiBzdHJva2UtbGluZWpvaW49InJvdW5kIi8%2BPHBhdGggZD0iTTE3OC45NDc4MywwLjAwMDY2NzU2MzMwNSBMMjQyLjcyNjQwOSwwLjAwMDY2NzU2MzMwNSBDMjUxLjY0MTc2MiwwLjAwMDY2NzU2MzMwNSAyNTcuMTc5MDc0LDcuMjU3MzEzNDUgMjU1LjA4NTA5MywxNi4xODk3NjU3IEwyMjUuMzk2MjcxLDE0My42NzU0MTYgQzIyMy4zMDA3MzYsMTUyLjU3Njc3NyAyMTQuMzgwNzE5LDE1OS44Mzk2NDEgMjA1LjQ2NjkyMiwxNTkuODM5NjQxIEwxNDEuNzE3ODc5LDE1OS44Mzk2NDEgQzEzMi43OTc4NjMsMTU5LjgzOTY0MSAxMjcuMjY1MjE1LDE1Mi41NzY3NzcgMTI5LjM1NDUzMiwxNDMuNjc1NDE2IEwxNTkuMDQ4MDE3LDE2LjE4OTc2NTcgQzE2MS4xMjQ4OTksNy4yNTczMTM0NSAxNzAuMDI3ODE0LDAuMDAwNjY3NTYzMzA1IDE3OC45NDc4MywwLjAwMDY2NzU2MzMwNSIgZmlsbD0iIzAwN0I4NCIgc3Ryb2tlPSIjZmZmIiBzdHJva2Utd2lkdGg9IjgiIHN0cm9rZS1saW5lam9pbj0icm91bmQiLz48L3N2Zz4%3D" alt="China UnionPay" height="23"> <img src="https://img.shields.io/badge/Alipay-1677FF?style=flat&amp;logo=alipay&amp;logoColor=white" alt="Alipay" height="23"> <img src="https://img.shields.io/badge/Vipps-FF5B24?style=flat&amp;logo=data:image/svg%2Bxml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHZpZXdCb3g9IjQzLjUgMjEuNSAyNyAyNyI%2BPHBhdGggZD0iTTU3LjMsNDAuNmMzLjcsMCw1LjgtMS44LDcuOC00LjRjMS4xLTEuNCwyLjUtMS43LDMuNS0wLjlzMS4xLDIuMywwLDMuN2MtMi45LDMuOC02LjYsNi4xLTExLjMsNi4xIGMtNS4xLDAtOS42LTIuOC0xMi43LTcuN2MtMC45LTEuMy0wLjctMi43LDAuMy0zLjRzMi41LTAuNCwzLjQsMUM1MC41LDM4LjMsNTMuNSw0MC42LDU3LjMsNDAuNnogTTY0LjIsMjguM2MwLDEuOC0xLjQsMy0zLDMgcy0zLTEuMi0zLTNzMS40LTMsMy0zQzYyLjgsMjUuMyw2NC4yLDI2LjYsNjQuMiwyOC4zeiIgZmlsbD0iI2ZmZiIvPjwvc3ZnPg%3D%3D" alt="Vipps" height="23"> <img src="https://img.shields.io/badge/SEPA-2D4B9A?style=flat&amp;logo=sepa&amp;logoColor=white" alt="SEPA" height="23"> <img src="https://img.shields.io/badge/Bitcoin-F7931A?style=flat&amp;logo=bitcoin&amp;logoColor=white" alt="Bitcoin" height="23"></p>

## Teşekkürler ve yasal bilgiler

OpenRA, XCC Mixer, ModEnc wiki, Project Perfect Mod, Command & Conquer ve Red Alert'in
kaynak kodlarını GPL lisansıyla yayımlayan EA, World-Altering Editor, Final Alert, YRpp,
Ares, Phobos ve daha nicelerine teşekkürler.

[GPLv3](LICENSE-GPL) ile lisanslanmıştır. Bu depo hiçbir oyun dosyası içermez.
Command & Conquer ve Red Alert, Electronic Arts Inc. şirketinin ticari markalarıdır.
Ekran görüntülerindeki oyun grafikleri Electronic Arts'a aittir. VERA20K, Electronic Arts
ile bağlantılı değildir ve Electronic Arts tarafından desteklenmemektedir.

[README.md](README.md) dosyasının çevirisidir; lütfen İngilizce sürümle güncel tutun.
