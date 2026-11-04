<img src="docs/images/new-conscirpt-hero-image.png" alt="VERA20K kapak görseli" width="100%">

<p align="center" dir="ltr">
  <a href="README.sv.md" lang="sv">Svenska</a> · <a href="README.zh-CN.md" lang="zh-CN">简体中文</a> · <a href="README.de.md" lang="de">Deutsch</a> · <a href="README.ar.md" lang="ar" dir="rtl">العربية</a> · <a href="README.es.md" lang="es">Español</a> · <a href="README.ja.md" lang="ja">日本語</a> · <a href="README.ru.md" lang="ru">Русский</a> · <a href="README.th.md" lang="th">ไทย</a> · <strong>Türkçe</strong> · <a href="README.md" lang="en">English</a>
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

**Daha fazla bakımcıya ihtiyacımız var.** PR'ları incelemeye yardım etmek, yeni katkıda
bulunanlara yol göstermek veya motorun bir bölümünün sorumluluğunu almak isterseniz, bunu bize
[Discord](https://discord.gg/kmjRUn5m5F) üzerinden ya da QQ grubunda (1097193563) bildirin.

**PR göndermekten çekinmeyin.** Mükemmel olması gerekmiyor; gerisi için biz yardımcı oluruz.

<img src="docs/images/vera20k-screenshots.png" alt="VERA20K çatışma ayarları ekranı ve oyun içi görünüm" width="100%">

## Projenin hedefleri

1. Orijinal Red Alert 2: Yuri's Revenge'in oynanışını, görsellerini ve atmosferini korumak.
2. 500×500 haritalarda **30 oyuncuya** ve **20.000 birime** kadar daha büyük savaşları desteklemek.
3. Bilinen RTS özelliklerini ve daha önce hiç görülmemiş bazı özellikleri eklemek.
4. Entegre çok oyunculu istemci

Güncel öncelikler ve gelecek planları için [yol haritasına](https://github.com/orgs/YuriPlanet/projects/1) bakın.

## İndir ve oyna

Windows, macOS veya Linux yükleyicisini
[sürümler sayfasından](https://github.com/YuriPlanet/vera20k/releases) indirin. Yükleyici,
Yuri's Revenge kopyanızı bulur ve VERA20K'yi yeni bir klasöre kurar.

Tek oyunculu YR çatışmalarını veya senaryolarını bir CnCNet istemcisinden başlatmak için
[çevrimdışı istemci kurulumuna](integrations/cncnet/README.md) bakın.

## Katkıda bulunma

Her alanda yardımınızı bekliyoruz; en çok yardıma ihtiyaç duyan alanlar aşağıda. Her biri yol
haritasındaki sayfasına bağlanır; orada süren çalışmayı ve sonraki adımı bulabilirsiniz:

- **[Oynanış ve çatışma](https://github.com/YuriPlanet/vera20k/issues/3830):** birim davranışı, hasar ve çatışma maçlarının güvenilirliği.
- **[Performans](https://github.com/YuriPlanet/vera20k/issues/3831):** büyük savaşlarda ve eski donanımda kare hızı ve tepki süresi.
- **[Çok oyunculu mod](https://github.com/YuriPlanet/vera20k/issues/3832):** çevrimiçi maçlara katılma, maç başlatma ve maçları sürdürme, ayrıca ortak oyunun güvenilirliği.
- **[Arayüz ve ses](https://github.com/YuriPlanet/vera20k/issues/3833):** menüler, metinler, çeviriler ve ses.
- **[Senaryolar](https://github.com/YuriPlanet/vera20k/issues/3834):** görevler başlatılabiliyor, ancak sonuna kadar oynanabilmeleri için daha çalışma gerekiyor: hedefler, tetikleyiciler, betikli geçişler, kaydetme/yükleme ve sonraki göreve geçiş.
- **[Mod uyumluluğu](https://github.com/YuriPlanet/vera20k/issues/3835):** orijinal oyunun varsayılanlarını değiştirmeden Ares ve Phobos özellikleri.
- **[Test](https://github.com/YuriPlanet/vera20k/issues?q=is%3Aopen%20label%3Aparity):** VERA20K'yi orijinal oyunla yan yana oynayıp farklı oynanan yerleri bildirmek.

Katkıda bulunanlar motorun belirli alanlarının sorumluluğunu üstlenebilir ve VERA20K'yi
ileriye taşıyan değişiklikleri geniş bir özgürlükle yapabilir.

Bakımcılar inceleme, entegrasyon ve birleştirme konusunda yardımcı olur. Katkınız tamamen
size atfedilir.

Tartışmalar [Discord](https://discord.gg/kmjRUn5m5F) üzerinden ve QQ grubunda (1097193563) yürür.

## Derleme ve çalıştırma

Gerekenler:

- **Rust ve Git:** rustup ile kurulmuş [Rust](https://rustup.rs/) 1.97 veya üzeri ve
  [Git](https://git-scm.com/).
- **Derleme araçları:** Windows'ta, Rust kurulum programının kurmayı önerdiği Visual Studio C++
  derleme araçları; macOS'te `xcode-select --install`; Debian ve Ubuntu'da
  `sudo apt install build-essential libasound2-dev libxkbcommon-x11-0 pkg-config`.

```sh
git clone https://github.com/YuriPlanet/vera20k.git
cd vera20k
cp config.toml.example config.toml
# Çalıştırmadan önce config.toml dosyasını düzenleyip ra2_dir değerini oyun klasörünüz olarak ayarlayın:
cargo run --release --bin vera20k
```

## Nasıl çalışıyoruz

Kodun büyük bölümünü yapay zekâ kodlama ajanları yazıyor. Ajanlar özgün motoru Ghidra ile
inceliyor, ardından davranışını Rust'a aktarıyor ve [karşılaştırma araçları](tools/native_oracle.md)
ile oyun testleri kullanarak kontrol ediyor. Ajanlar [AGENTS.md](AGENTS.md) dosyasını izler.

## Özelleştirme

Harita kapasitesi derleme bayrağı olmadan otomatik seçilir: `Size` genişliği ile yüksekliğinin toplamı en fazla 1024 olabilir.

VERA20K varsayılan olarak orijinal oyun gibi oynanır. Ares gibi modlardan gelen eklentiler dahil
diğer her yeni özellik, şu yollardan biriyle açılana kadar kapalı kalmalıdır:

- **Bir VERA oyuncu seçeneği**: **Options** altında bulunur ve `VERA.INI` dosyasının `[VERA]` bölümüne kaydedilir.
- **Bir derleme seçeneği**: örneğin desteklenen [Ares eklentileri](docs/ares.md) için `--features ares`.
- **Bir INI anahtarı**: bir mod ya da harita tarafından ayarlanır.

Ses, grafik ve kontrol ayarları `VERA.INI` dosyasına kaydedilir. Oynanış kuralları lobi
ayarlarından, modlardan veya haritalardan gelir.

## Destek

<a href="https://paypal.me/vera20k"><img src="https://img.shields.io/badge/Donate_via_PayPal-FFC439?style=flat&amp;logo=paypal&amp;logoColor=003087" alt="VERA20K'yi PayPal üzerinden destekleyin" height="23"></a>

Alınan tüm bağışlar,
[TIBTRE katkı defterine](https://github.com/YuriPlanet/tibtre-ledger) göre katkıda
bulunanlara dağıtılır.

Katkıda bulunanlara ödeme yöntemleri: PayPal, China UnionPay, Alipay, Vipps, SEPA ve Bitcoin.

## Teşekkürler ve yasal bilgiler

OpenRA, XCC Mixer, ModEnc wiki, Project Perfect Mod, Command & Conquer ve Red Alert'in
kaynak kodlarını GPL lisansıyla yayımlayan EA, World-Altering Editor, Final Alert, YRpp,
Ares, Phobos ve daha nicelerine teşekkürler.

Telif hakkı © 2026 Halvor Nikolai Rusten ve VERA20K'ya katkıda bulunanlar.
[Ek koşullarla](LEGAL.md#additional-terms) birlikte [GPLv3](LICENSE-GPL) ile lisanslanmıştır.
Bu depo hiçbir oyun dosyası içermez.
Command & Conquer ve Red Alert, Electronic Arts Inc. şirketinin ticari markalarıdır.
Ekran görüntülerindeki ve uygulama simgesindeki (Red Alert 2'nin kırmızı yıldızı) oyun grafikleri
Electronic Arts'a aittir. VERA20K, Electronic Arts ile bağlantılı değildir ve Electronic Arts
tarafından desteklenmemektedir.

**VERA20K'yı paylaşmadan, satmadan veya fork'lamadan önce [yasal bildirimi](LEGAL.md) okuyun.**

## Belgeler

- [WebAssembly önizlemesi](docs/wasm.md): tarayıcıda çalışan, isteğe bağlı tek oyunculu bir derleme.
- [INI profilleri](docs/ini-roots.md): kural, grafik ve yapay zekâ dosyalarını seçme.
- [Ares](docs/ares.md), [Phobos](docs/phobos.md) ve [NPatch](docs/npatch.md): eklenti desteği ve sınırlamaları.
- [Motora genel bakış](docs/index.md): kodun düzeni ve motorun parçalarının nasıl bir araya geldiği.
- [Geliştirici araçları](tools/README.md): derlemeler, testler ve oyun kaynaklarının incelenmesi.
- [Harita testleri](tools/map_observation.md): betikli test senaryoları, ekran görüntüleri ve maç gözlemleri.
- [Orijinalle karşılaştırmalar](tools/native_oracle.md): davranışı orijinal oyunla karşılaştırma.
- [Ghidra iş akışı](docs/ghidra-workflow.md): orijinal motoru inceleme.

[README.md](README.md) dosyasının çevirisidir; lütfen İngilizce sürümle güncel tutun.
