---
layout: default
title: Harekât bildirimindeki anahtarlar
lang: tr
---

# Harekât bildirimindeki anahtarlar

[English](../campaign-manifest-keys.md) · [简体中文](../zh-CN/campaign-manifest-keys.md) · [Deutsch](../de/campaign-manifest-keys.md) · [العربية](../ar/campaign-manifest-keys.md) · [Русский](../ru/campaign-manifest-keys.md) · [Svenska](../sv/campaign-manifest-keys.md) · [ไทย](../th/campaign-manifest-keys.md) · **Türkçe**

`<id>` yerine görev kimliğini yazın. Mantıksal anahtarlar `Yes`, `No`, `True`, `False`, `1` ve `0` kabul eder. Tablolar bu dosyadan açılan görevlerin varsayılan değerlerini gösterir. Tire, anahtarın atlandığını gösterir. Kullanmadığınız isteğe bağlı satırları tamamen kaldırın. [Kurulum, örnekler ve kontrol](campaign-manifest.md)

## `[Campaign]`

| Anahtar | Değerler | Varsayılan | Kullanıldığı yer ve etkisi |
| --- | --- | --- | --- |
| `Title` | Metin | Seri klasörü | Oda modları listesi, kurulu serileri okurken bu adı gösterir. Örnek `Title=Broken Line`. |
| `Title.<locale>` | Metin | `Title` | Seri listesi, arayüz diline uygun başlığı seçer. Örnek `Title.tr=Kırık Hat`. Sonra temel dil ve `Title` denenir. |

## `[Stage <id>]`

| Anahtar | Değerler | Varsayılan | Kullanıldığı yer ve etkisi |
| --- | --- | --- | --- |
| `Map` | Göreli yol | Zorunlu | Oda doğrulaması ve harita yükleyici bu dosyayı seri klasöründen açar. Örnek `Map=maps/opening.map`. Çözümlenen yol seri klasöründe kalmalı. |
| `MapSha256` | 64 onaltılık rakam | — | Harita kontrolünde dosyanın SHA-256 özeti bu değerle karşılaştırılır. Anahtar atlandığında da oyuncular kendi dosyalarını karşılaştırır. |
| `MinPlayers` | 1–128 | `1` | Başlatmadan önce en az bu kadar oyuncu gerekir. İki rol de şartsa `MinPlayers=2` kullanın. İzleyiciler sayılmaz. |
| `MaxPlayers` | 1–128 | Rol ve oda sınırı | Odaya girişte bu değer, mevcut rol sayısı ve oda kapasitesinden en küçüğü uygulanır. Rolü olmayan görev tek oyuncu alır. |
| `Title` | Metin | `ID` | Görev seçici ve sonuç ekranı bu adı gösterir. Örnek `Title=Yarma`. |
| `Title.<locale>` | Metin | `Title` | Görev seçimi ve sonuçlar, arayüz diline uygun başlığı kullanır. Örnek `Title.tr=Yarma`. Yedek sırası `[Campaign] Title.<locale>` ile aynıdır. |

## `[Roles <id>]`

| Anahtar | Değerler | Varsayılan | Kullanıldığı yer ve etkisi |
| --- | --- | --- | --- |
| `RoleId=HouseId` | Haritadaki taraf kimliği | Tek oyuncu için `[Basic] Player` | Kurulum ve başlangıçta rolü haritanın `[Houses]` listesindeki bir tarafa bağlar. `Lead=Americans` ve `Support=Americans` aynı birincil tarafı paylaşır. Her rol tek oyuncuya verilir. |

## `[Grants <id>]`

| Anahtar | Değerler | Varsayılan | Kullanıldığı yer ve etkisi |
| --- | --- | --- | --- |
| `RoleId.HouseId` | `FullControl` / `UnitsOnly` | — | Başlangıçta `FullControl` başka bir tarafın taktik ve tam yetki isteyen komutlarını açar. `UnitsOnly` taktik komutları açar. Örnek `Lead.Russians=FullControl`. Birincil tarafa özgü komutlar rolün kendi tarafına aittir. |

## `[Routes <id>]`

| Anahtar | Değerler | Varsayılan | Kullanıldığı yer ve etkisi |
| --- | --- | --- | --- |
| `BranchId=StageId` | Tanımlanmış görev kimliği | — | Dosya okunurken hedef görev aranır. `Coast=Finale` hedefi devam menüsüne ekler. `Retry` yalnızca geçerli görevi gösterebilir. Anahtar atlandığında da yeniden deneme giriş durumunu kullanır. |

## `[CarryOver <id>]`

| Anahtar | Değerler | Varsayılan | Kullanıldığı yer ve etkisi |
| --- | --- | --- | --- |
| `Globals` | `Yes` / `No` | `No` | Devam sırasında genel tetikleyici değişkenlerini saklar ve sonraki haritanın ilk karesinden önce geri yükler. |
| `Countdown` | `Yes` / `No` | `No` | Görev sonunda kalan süreyi saklar. Burada `Countdown=Yes`, hedef haritada `[Basic] TimerInherit=Yes` yazın. |
| `MapStage` | `Yes` / `No` | `No` | Görev sonunda haritanın sayısal harekât aşamasını saklar. `MapStage=Yes` bunu sonraki haritaya taşır. Görev kimliği güzergâhtan alınır. |
| `House.<id>` | `Credits` / `Difficulty` / `Credits,Difficulty` | — | Görev sonunda belirtilen tarafın parasını, zorluğunu veya ikisini aktarır. `House.Americans=Credits,Difficulty` iki haritada da `Americans` gerektirir. Hedef haritanın `CarryOverMoney` ve `CarryOverCap` değerleri tutarı belirler. |

## `[Rules <id>]`

| Anahtar | Değerler | Varsayılan | Kullanıldığı yer ve etkisi |
| --- | --- | --- | --- |
| `Bootstrap` | `Campaign` | `Campaign` | Başlangıçta haritanın taraflarını kurar. Açıkça belirtmek için `Bootstrap=Campaign` yazın. |
| `AllowWinTags` | `RequireClear` / `Ignore` | `RequireClear` | Zafer kontrolünde `RequireClear`, AllowWin etiketi sayacının sıfırlanmasını bekler. `Ignore` bu sayacı atlar. Sonuç zamanlayıcısı her iki durumda işler. |
| `ComputerParanoid` | `Disabled` / `React` | `Disabled` | Düşman veya ittifak yeniden değerlendirilirken `React`, Computer_Paranoid yolunu açar. Kural ve taraf denetimleri geçerlidir. |
| `Crates` | `Keep` / `Skip` | `Keep` | Kaplamalar yüklenirken `Keep` haritaya yerleştirilen sandıkları tutar. `Skip` onları atlar. Zamanlı sandıklar için `UsesMatchCrates` kullanılır. |
| `AiBase` | `RetainAuthoredNodes` / `ReplaceUnavailableNodes` | `RetainAuthoredNodes` | Yapay zekâ üs planını işlerken `RetainAuthoredNodes` yerleştirilmiş düğümleri korur. `ReplaceUnavailableNodes` kullanılamayan veya sürekli reddedilen konumları değiştirebilir. |
| `DoubleOwned` | `OwnerMask` / `AllCountries` | `OwnerMask` | Fabrika seçerken `OwnerMask` normal ülke maskesini kullanır. `AllCountries`, `DoubleOwned=Yes` ayarlı türlerin maskesini genişletir. |
| `TeamCreationLimitScope` | `AllHouses` / `PerHouse` | `AllHouses` | Takım kurulurken TeamType `Max=` sınırı için tüm taraflardaki veya sadece kuran taraftaki mevcut takımları sayar. |
| `CountsPlayerControlAsHuman` | `No` | `No` | Başlangıçta oyuncu tarafları kabul edilen rollerden belirlenir. Bunu `CountsPlayerControlAsHuman=No` ile açıkça yazabilirsiniz. |
| `AllowsConstructionYardRedeploy` | `Yes` / `No` | `No` | İnşaat alanını toplama komutu gelince uygun binaların araca dönüşmesine izin verir. Bina koşulları ve maç seçeneği yine denetlenir. |
| `AllowsIdleObjectSleep` | `Yes` / `No` | `No` | Kare güncellemesinde tetikleyici ve onarım koşullarını sağlayan boşta nesneleri uyutabilir. |
| `AllowsOwnerlessTerrainLogic` | `Yes` / `No` | `Yes` | Arazi güncellemesinde kimlik denetiminin normalde atladığı sahipsiz nesneleri de işler. |
| `UsesCountryDifficultyBiases` | `Yes` / `No` | `No` | Tarafa zorluk uygulanırken seçilen ülkenin çarpanlarını da kullanır. |
| `SkipsLocalPlayerAuthoredBuildings` | `Yes` / `No` | `No` | Bina yükleyici bu anahtarı sadece yerel çalıştırmada kullanır. Lockstep haritaya konmuş binaları yükler. Binayı kaldırmak için haritayı düzenleyin. |
| `UsesManagedAiBase` | `Yes` / `No` | `No` | Oyun sırasında uygun yapay zekâ taraflarına otomatik inşa, yer seçimi ve konuşlandırma verir. |
| `UsesAiEconomy` | `Yes` / `No` | `No` | Yapay zekâ ekonomisinde maden ödemesi ve uygun bina satışları için çok oyunculu kuralları kullanır. |
| `RunsAiStrategy` | `Yes` / `No` | `No` | Strateji güncellemesinde yapay zekânın düzenli düşman aramasını ve kendi inşa kararlarını vermesini sağlar. |
| `UsesHouseTeamRules` | `Yes` / `No` | `No` | Yapay zekâ takımlarını yüklerken ve etkinleştirirken taraf bazlı tetikleyici yolunu seçer. Bu yol zorluğu ve TechLevel koşullarını taraf ve takıma göre denetler. |
| `UsesCampaignDiscovery` | `Yes` / `No` | `Yes` | Görüş ve hedef seçiminde harekât keşif kurallarını kullanır. Tek istemcili keşif yolu yerel çalıştırma gerektirir. |
| `UsesCampaignBaseDefense` | `Yes` / `No` | `Yes` | Üs saldırıya uğradığında vurulan nesnenin ve uygun savunucuların tepki biçimini harekât kurallarına göre seçer. |
| `UsesDirectedHunterSeekerTargeting` | `Yes` / `No` | `No` | Ares Hunter Seeker ateşlendiğinde yapay zekâ atanan düşmanı kullanabilir. `RandomOnly` elverirse oyuncu tarafı pasif olmayan hedeflere öncelik verebilir. |
| `UsesMatchCrates` | `Yes` / `No` | `No` | Oyun sırasında çok oyunculu sandık zamanlayıcılarını ve toplama denetimlerini kullanır. Otomatik sandık çıkması için maçtaki sandık seçeneği açılmalı. |
| `UsesEliminationResults` | `Yes` / `No` | `No` | Sonuç kontrolünde tarafların elenmesine göre karar verir ve temizliği uygular. Varsayılan değer kazanma ve kaybetme tetikleyicilerini kullanmaya izin verir. |
| `UsesRoomOptions` | `Yes` / `No` | `No` | Görev kurulurken hasatçı dokunulmazlığı ile maden büyümesi ve yayılması için oda başlangıç yolunu kullanır. |
| `UsesCampaignDiplomacy` | `Yes` / `No` | `Yes` | Diplomasi düğmesi harekât brifingini açar. Pasif veya sivil tarafların diplomasi işlemlerinde de harekât kuralları uygulanır. |
| `AllowsManualAlliances` | `Yes` / `No` | `No` | İttifak komutu gelince, ittifak kuralları ve oyuncu yetkisi uygunsa işleme alınmasına izin verir. |
| `ModMapAdditions` | `Yes` / `No` | `No` | Grafik istemci haritayı yüklerken etkin modun harita eklerini kullanabilir. Oyuncular aynı mod dosyalarını kullanmalı. Yerel denetim için gerekli değerleri haritaya koyun. |
| `SharedAlliedVictory` | `Yes` / `No` | `No` | Lockstep sonucu elemeyle belirlenirken koşulları sağlayan hayatta kalan müttefikler zaferi paylaşabilir. |
| `SkipsNetworkScoreDialog` | `Yes` / `No` | `No` | Normal çok oyunculu puan ekranı çağrıldığında ekranı atlar. Harekâtın devamı ayrı sonuç ekranını kullanır. |
| `ParanoidIgnoresAlliesAllowed` | `Yes` / `No` | `No` | `ComputerParanoid=React` ittifak denetimine ulaştığında yapay zekâ `AlliesAllowed` engelini aşabilir. Oyuncu yetkileri ayrı denetlenir. |
| `NetworkDiplomacyBriefing` | `Yes` / `No` | `No` | Diplomasi düğmesi brifingi ve salt okunur rol yetkilerini açar. Örnek `NetworkDiplomacyBriefing=Yes` ve haritada `[Basic] Briefing` metni. |
| `StartingHouseRoleAlliances` | `Yes` / `No` | `No` | Taraflar oda yerlerinden kurulurken aynı insan veya yapay zekâ sınıfındaki tarafları müttefik yapar. Haritada sabit taraflar varsa açılış ittifaklarını haritada tanımlayın. |
| `AresPsychicRevealExclusions` | `Yes` / `No` | `No` | Lockstep sırasında Ares'in psişik açığa çıkarma etkisi çalışırken rekabetçi yolun haritaya göre belirlenen üç karesini görünmez bırakır. |
| `DissolveUnfilledTeams` | `Yes` / `No` | `No` | Yapay zekâ takımları güncellenirken hiç dolmamış boş takım `DissolveUnfilledTeamDelay` süresinden sonra dağıtılır. |
