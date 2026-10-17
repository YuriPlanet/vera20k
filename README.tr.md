<img src="docs/images/new-conscirpt-hero-image.png" alt="VERA20k kapak görseli" width="100%">

<p align="center" dir="ltr">
  <a href="README.sv.md" lang="sv">Svenska</a> · <a href="README.zh-CN.md" lang="zh-CN">简体中文</a> · <a href="README.de.md" lang="de">Deutsch</a> · <a href="README.ar.md" lang="ar" dir="rtl">العربية</a> · <a href="README.ru.md" lang="ru">Русский</a> · <a href="README.th.md" lang="th">ไทย</a> · <strong>Türkçe</strong> · <a href="README.md" lang="en">English</a>
  &nbsp;&nbsp;
  <a href="https://github.com/YuriPlanet/vera20k/actions/workflows/macos.yml?query=branch%3Amain" title="Son macOS kütüphane testi çalıştırması (elle başlatılır)"><img src="https://github.com/YuriPlanet/vera20k/actions/workflows/macos.yml/badge.svg?branch=main" alt="macOS kütüphane testleri" height="20" align="middle"></a>
  <a href="https://github.com/YuriPlanet/vera20k/actions/workflows/linux.yml?query=branch%3Amain" title="Son Linux kütüphane testi çalıştırması (elle başlatılır)"><img src="https://github.com/YuriPlanet/vera20k/actions/workflows/linux.yml/badge.svg?branch=main" alt="Linux kütüphane testleri" height="20" align="middle"></a>
  <a href="https://github.com/YuriPlanet/vera20k/actions/workflows/windows.yml?query=branch%3Amain" title="Son Windows kütüphane testi çalıştırması (elle başlatılır)"><img src="https://github.com/YuriPlanet/vera20k/actions/workflows/windows.yml/badge.svg?branch=main" alt="Windows kütüphane testleri" height="20" align="middle"></a>
  <a href="https://discord.gg/kmjRUn5m5F"><img src="https://img.shields.io/badge/Discord-Join-5865F2?style=flat&amp;logo=discord&amp;logoColor=white" alt="Discord'a katılın" height="20" align="middle"></a>
  <img src="https://img.shields.io/badge/QQ_grubu-1097193563-1EBAFC?style=flat&amp;logo=qq&amp;logoColor=white" alt="QQ grubu" height="20" align="middle">
</p>

# VERA20k

VERA20k, özgün oyun motoru `gamemd.exe`'nin topluluk öncülüğünde geliştirilen yeniden
yazımıdır. Orijinal oyun dosyalarını kullandığı için kendi Red Alert 2: Yuri's Revenge
kopyanıza ihtiyacınız var.

VERA20k oyuncular tarafından, oyuncular için yapılıyor ve projenin nereye gideceğine son sözü
oyuncular söylüyor.

<img src="docs/images/vera20k-screenshots.png" alt="VERA20k çatışma ayarları ekranı ve oyun içi görünüm" width="100%">

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

## Derleme ve çalıştırma

Gerekenler:

- **Oyun:** Red Alert 2: Yuri's Revenge.
- **Rust ve Git:** rustup ile kurulmuş güncel kararlı [Rust](https://rustup.rs/) sürümü ve
  [Git](https://git-scm.com/).
- **Derleme araçları:** Windows'ta, Rust kurulum programının kurmayı önerdiği Visual Studio C++
  derleme araçları; macOS'te `xcode-select --install`; Debian ve Ubuntu'da
  `sudo apt install build-essential libasound2-dev pkg-config`.
- **Grafik:** Vulkan, DirectX 12 ya da Metal destekleyen bir GPU.

VERA20k, Windows, Linux ve macOS üzerinde oynandı.

```sh
git clone https://github.com/YuriPlanet/vera20k.git
cd vera20k
cp config.toml.example config.toml
# Çalıştırmadan önce config.toml dosyasını düzenleyip ra2_dir değerini oyun klasörünüz olarak ayarlayın:
cargo run --release --bin vera20k
```

`ra2_dir` yolunu ters eğik çizgi yerine eğik çizgiyle yazın, örneğin `C:/Games/RA2`. Oynamak için
`--release` kullanın; debug derlemeleri çok yavaş. Günlük dosyası `logs/ra2.log` konumundadır.

## Nasıl çalışıyoruz

Kodun büyük bölümünü yapay zekâ kodlama ajanları yazıyor. Ajanlar özgün motoru Ghidra ile
inceliyor, ardından davranışını Rust'a aktarıyor ve [karşılaştırma araçları](tools/native_oracle.md)
ile oyun testleri kullanarak kontrol ediyor. Ajanlar [AGENTS.md](AGENTS.md) dosyasını izler.

## Özelleştirme

İsteğe bağlı özellikler (ör. Ares özellikleri) INI dosyaları üzerinden yapılandırılabilmeli
ve uygun olduğunda istemciden açılıp kapatılabilmelidir.

## Katkıda bulunma

Yardımlarınızı bekliyoruz. Kod yazabilir, motoru yeniden düzenleyebilir, oyunu test edebilir,
fikirlerinizi paylaşabilir veya orijinal oyunla yan yana oynayıp neyin yanlış hissettirdiğini
bize anlatabilirsiniz. Bir PR açın, gerisini biz hallederiz.
Tartışmalar [Discord](https://discord.gg/kmjRUn5m5F) üzerinden ve QQ grubunda (1097193563) yürür.

Oyun mantığı `src/sim/`, çizim `src/render/`, menüler ve girdi ise `src/app/` ve `src/ui/`
içinde; `tools/` altındaki Python araçlarına ihtiyacınız yok.

Değişikliklerinizi oyunda denemek için `cargo run --profile fast --bin vera20k` çalıştırın.
`--release` gibi optimize edilmiştir ama yalnızca değiştirdiğiniz kısmı yeniden derler, böylece
oyuna daha çabuk dönersiniz. Performansı `--release` ile ölçün.

Katkılar da projenin geri kalanı gibi GPLv3 ile lisanslanır; CLA yoktur.

## Destek <a href="https://paypal.me/vera20k"><img src="https://img.shields.io/badge/PayPal-Donate-0070BA?style=flat&amp;logo=paypal&amp;logoColor=white" alt="VERA20k'yi PayPal üzerinden destekleyin" height="20" align="middle"></a>

[VERA20k'yi PayPal üzerinden destekleyin](https://paypal.me/vera20k). Alınan tüm bağışlar,
[TIBTRE katkı defterine](https://github.com/YuriPlanet/tibtre-ledger) göre katkıda
bulunanlara dağıtılır.

Katkıda bulunanlara ödemeler PayPal, UnionPay (Çin), Alipay, Vipps, SEPA banka havalesi veya
Bitcoin üzerinden yapılır.

## Teşekkürler ve yasal bilgiler

OpenRA, XCC Mixer, ModEnc wiki, Project Perfect Mod, Command & Conquer ve Red Alert'in
kaynak kodlarını GPL lisansıyla yayımlayan EA, World-Altering Editor, Final Alert, YRpp,
Ares, Phobos ve daha nicelerine teşekkürler.

[GPLv3](LICENSE-GPL) ile lisanslanmıştır. Bu depo hiçbir oyun dosyası içermez.
Command & Conquer ve Red Alert, Electronic Arts Inc. şirketinin ticari markalarıdır.
Ekran görüntülerindeki oyun grafikleri Electronic Arts'a aittir. VERA20k, Electronic Arts
ile bağlantılı değildir ve Electronic Arts tarafından desteklenmemektedir.

[README.md](README.md) dosyasının çevirisidir; lütfen İngilizce sürümle güncel tutun.
