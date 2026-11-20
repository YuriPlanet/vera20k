---
layout: default
title: Harekât serileri
lang: tr
---

# Harekât serileri

[English](../campaign-manifest.md) · [简体中文](../zh-CN/campaign-manifest.md) · [Deutsch](../de/campaign-manifest.md) · [العربية](../ar/campaign-manifest.md) · [Русский](../ru/campaign-manifest.md) · [Svenska](../sv/campaign-manifest.md) · [ไทย](../th/campaign-manifest.md) · **Türkçe**

`campaigns/<seri>/campaign.ini` dosyası serideki her görevin haritasını belirler. Kuralları, rolleri, güzergâhları ve sonraki göreve aktarılacak durumu da burada yazabilirsiniz. Başlamak için tek harita yeterlidir.

Bir görevi oda açmadan sınamak için `asset campaign-check` çalıştırın. Oynamak için tek kişilik bir oda açabilir veya birden fazla oyuncuya rol verebilirsiniz. İnternet oyunu, harekât odalarını destekleyen bir lobi gerektirir. Normal **Harekât** menüsü oyunun `battlemd.ini` listesini kullanır.

[Anahtar başvurusu](campaign-manifest-keys.md) her bölümün değerlerini, varsayılanlarını ve kullanıldığı yeri açıklar.

## Tek görevle başlayın

`campaign.ini` dosyasını UTF‑8 olarak `campaigns/broken-line/` içine kaydedin. Haritayı bu klasördeki `maps/opening.map` konumuna koyun. Bildirim dosyası en fazla 64 KiB olabilir. Harita yolunu seri klasörüne göre yazın ve ayırıcı olarak `/` kullanın.

```ini
[Stage Opening]
Map=maps/opening.map
```

Rolü olmayan görev `[Basic] Player` tarafını kullanır ve tek kişilik yer açar.

## Görevi oda açmadan sınama

```text
asset campaign-check campaigns/broken-line/campaign.ini Opening 120 --ra2-dir "/path/to/Yuri's Revenge"
```

Görev kimliğini ve sıfırdan büyük bir kare sayısını girin. Araç haritayı çalıştırır, tanılama kaydından yeniden kurar ve her karedeki durumu karşılaştırır. Bildirimde hata varsa dosya, bölüm, anahtar, satır ve düzeltme önerisi gösterir. Oda dosyaları eşleşmiyorsa haritanın yüklediği INI dosyalarını da karşılaştırın.

## Rol ekleme

Rol eklerken haritanın `[Houses]` listesindeki taraf adlarını kullanın.

```ini
[Roles Opening]
Lead=Americans
Support=Americans

[Grants Opening]
Lead.Russians=FullControl
Support.Russians=UnitsOnly
```

`Lead` ve `Support`, `Americans` tarafını paylaşır. `FullControl`, `Russians` üzerinde tam yetki isteyen komutları da açar. `UnitsOnly` taktik komutlara izin verir. Oyuncu sınırları için `MinPlayers` ve `MaxPlayers` kullanın. `[Routes Opening]` altında `Coast=Finale` tanımlanmış bir göreve geçiş verir. `[CarryOver Opening]` aktarılacak bilgileri seçer.

## Kurulum ve oda açma

Harekât klasörünü oyun dizinindeki veya etkin moddaki `campaigns/<seri>/` altına yerleştirin. Tüm oyuncular aynı oyun derlemesini, kuralları ve harekât dosyalarını kullanmalı. Kopyalarken klasör yapısını koruyun ve haritaların yüklediği INI dosyalarını da ekleyin.

1. Bir LAN odası açıp mod listesinden harekâtı seçin. **Standart çok oyunculu oyun** olağan modlara döner.
2. Görevi ve zorluğu seçin. Görevin rol listesi varsa **Rol** sütununda her oyuncuya ayrı bir rol verin. İki rol aynı tarafı yönetebilir.
3. Herkes hazır olduğunda başlatın. Oyun başlamazsa önce dosyaları, rol dağılımını ve görevin oyuncu sınırlarını kontrol edin.

İnternet odası için WOL lobisinin `campaign_rooms` desteğini bildirmesi ve harekât ayarlarını oyun başlarken ve izleyici katılırken iletmesi gerekir. Lobi desteğinin bulunmadığını bildirirse LAN kullanın. İzleyici bağlantısı rol almaz ve oy kullanmaz. Başlamadan önce odadaki normal seyirci yerlerini boşaltın.

Ara sahnede oyun, oyuncular aynı noktaya ulaşana kadar bekler. Görev sonunda **Diğer güzergâh** ekrandaki hedefi değiştirir. **Devam et** o hedefe oy verir. **Yeniden dene** görevi girişteki durumuyla başlatır. Geçiş için tüm oyuncular aynı seçeneği seçmeli.

[İngilizce INI biçimi ve tekrar oynatma kılavuzu](../campaign-manifest.md) · [İngilizce ağ protokolü](../Networking.md#multiplayer-campaign-rooms)
