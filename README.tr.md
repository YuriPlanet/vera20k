<img src="docs/images/new-conscirpt-hero-image.png" alt="VERA20k kapak görseli" width="100%">

<p align="center" dir="ltr">
  <a href="README.sv.md" lang="sv">Svenska</a> · <a href="README.zh-CN.md" lang="zh-CN">简体中文</a> · <a href="README.de.md" lang="de">Deutsch</a> · <a href="README.ar.md" lang="ar" dir="rtl">العربية</a> · <a href="README.ru.md" lang="ru">Русский</a> · <a href="README.th.md" lang="th">ไทย</a> · <strong>Türkçe</strong> · <a href="README.md" lang="en">English</a>
  &nbsp;&nbsp;
  <a href="https://github.com/YuriPlanet/vera20k/actions/workflows/macos.yml?query=branch%3Amain" title="Son macOS kütüphane testi çalıştırması (elle başlatılır)"><img src="https://github.com/YuriPlanet/vera20k/actions/workflows/macos.yml/badge.svg?branch=main" alt="macOS kütüphane testleri" height="20" align="middle"></a>
  <a href="https://github.com/YuriPlanet/vera20k/actions/workflows/linux.yml?query=branch%3Amain" title="Son Linux kütüphane testi çalıştırması (elle başlatılır)"><img src="https://github.com/YuriPlanet/vera20k/actions/workflows/linux.yml/badge.svg?branch=main" alt="Linux kütüphane testleri" height="20" align="middle"></a>
  <a href="https://github.com/YuriPlanet/vera20k/actions/workflows/windows.yml?query=branch%3Amain" title="Son Windows kütüphane testi çalıştırması (elle başlatılır)"><img src="https://github.com/YuriPlanet/vera20k/actions/workflows/windows.yml/badge.svg?branch=main" alt="Windows kütüphane testleri" height="20" align="middle"></a>
  <a href="https://discord.gg/kmjRUn5m5F"><img src="https://img.shields.io/badge/Discord-Join-5865F2?style=flat&amp;logo=discord&amp;logoColor=white" alt="Discord'a katılın" height="20" align="middle"></a>
</p>

# VERA20k

Red Alert 2: Yuri's Revenge — büyük çok oyunculu savaşlar için Rust ile yeniden yazıldı.

VERA20k, özgün oyun motoru `gamemd.exe`'nin yeniden yazımıdır. Orijinal oyun dosyalarını
kullandığı için kendi Red Alert 2: Yuri's Revenge kopyanıza ihtiyacınız var. Oyun, [Steam](https://store.steampowered.com/bundle/39394/)
ve [EA](https://www.ea.com/games/command-and-conquer/command-and-conquer-the-ultimate-collection/buy/pc)
üzerinde satılan *Command & Conquer The Ultimate Collection* paketinde bulunuyor.

VERA20k oyuncular tarafından, oyuncular için yapılıyor ve projenin nereye gideceğine son sözü
oyuncular söylüyor.

<img src="docs/images/vera20k-screenshots.png" alt="VERA20k çatışma ayarları ekranı ve oyun içi görünüm" width="100%">

## Projenin hedefleri

1. Orijinal Red Alert 2: Yuri's Revenge'in oynanışını, görsellerini ve atmosferini korumak.
2. Daha büyük haritalarda **30 oyuncuya** ve **20.000 birime** kadar daha büyük savaşları desteklemek.
3. Yeni RTS özellikleri eklemek.
4. Entegre çok oyunculu istemci

## Mevcut durum

**Geliştirmenin ilk aşamalarında.** Windows'ta basit bir yapay zekâya karşı yerel çatışmalar
oynanabiliyor. Orijinal ve rastgele oluşturulan haritalar, menüler, üs kurma, kaynak toplama,
çatışma ve kaydetme/yükleme mevcut, ancak hâlâ düzeltilecek ve tamamlanacak çok şey var.

Çok oyunculu mod, senaryolar ve orijinal yapay zekâ henüz yok. Uçaklar, zihin kontrolü, köprüler
ve çeşitli silahlar ile efektler üzerinde daha fazla çalışmamız gerekiyor. Henüz 30 oyunculu,
20.000 birimli savaşlar göstermedik.

## Derleme ve çalıştırma

Rust 1.88 veya üzeri, Vulkan, DirectX 12 ya da Metal destekleyen bir GPU ve kurulu oyun gerekiyor.
VERA20k, Windows, Linux ve macOS üzerinde oynandı.

```sh
git clone https://github.com/YuriPlanet/vera20k.git
cd vera20k
cp config.toml.example config.toml
# Çalıştırmadan önce config.toml dosyasını düzenleyip ra2_dir değerini oyun klasörünüz olarak ayarlayın:
cargo run --release --bin vera20k
```

Oynamak için `--release` kullanın; debug derlemeleri çok yavaş. Platforma göre kurulum ve
testleri çalıştırma bilgileri için [CONTRIBUTING.md](CONTRIBUTING.md#set-up) dosyasına bakın.

## Nasıl çalışıyoruz

Kodun büyük bölümünü yönlendirdiğim yapay zekâ kodlama ajanları yazıyor. Özgün motoru
Ghidra ile inceliyor, ardından davranışını Rust'a aktarıyor ve
[karşılaştırma araçları](tools/native_oracle.md) ile oyun testleri kullanarak kontrol ediyoruz.
Çalışma kurallarımız [AGENTS.md](AGENTS.md) dosyasında.

## Katkıda bulunma

Yardımlarınızı bekliyoruz. Kod yazabilir, oyunu test edebilir, belgeleri geliştirebilir veya
orijinal oyunla yan yana oynayıp neyin yanlış hissettirdiğini bize anlatabilirsiniz.
Yardım etmek için tersine mühendislik deneyimine ihtiyacınız yok.

[CONTRIBUTING.md](CONTRIBUTING.md) dosyasını okuyun,
[yeni başlayanlara uygun işlere](https://github.com/YuriPlanet/vera20k/labels/good%20first%20issue)
göz atın ya da [Discord](https://discord.gg/kmjRUn5m5F) üzerinden merhaba deyin.
[Mimariye genel bakış](https://yuriplanet.github.io/vera20k/tr/), motorun parçalarının nasıl bir araya geldiğini açıklıyor.

## Teşekkürler ve yasal bilgiler

OpenRA, XCC Mixer, ModEnc wiki, Project Perfect Mod, Command & Conquer ve Red Alert'in
kaynak kodlarını GPL lisansıyla yayımlayan EA, World-Altering Editor, Final Alert, YRpp,
Ares, Phobos ve daha nicelerine teşekkürler.

[GPLv3](LICENSE-GPL) ile lisanslanmıştır. Bu depo hiçbir oyun dosyası içermez.
Command & Conquer ve Red Alert, Electronic Arts Inc. şirketinin ticari markalarıdır.
Ekran görüntülerindeki oyun grafikleri Electronic Arts'a aittir. VERA20k, Electronic Arts
ile bağlantılı değildir ve Electronic Arts tarafından desteklenmemektedir.

[README.md](README.md) dosyasının çevirisidir; lütfen İngilizce sürümle güncel tutun.
