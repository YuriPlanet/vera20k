---
layout: default
title: VERA20k Motoru
lang: tr
---

# VERA20k

[Svenska](../sv/) · [简体中文](../zh-CN/) · [Deutsch](../de/) · [العربية](../ar/) · [Русский](../ru/) · [ไทย](../th/) · **Türkçe** · [English](../)

Red Alert 2: Yuri's Revenge — Rust ile sıfırdan yeniden yazıldı.

Bu sayfa kodun kısa bir haritasıdır. Kodla çelişirse doğru olan koddur. Her modülün başındaki `//!` yorumu, modülün ne yaptığını ve nelere bağımlı olabileceğini açıklar.

## Kod

- `sim/` — tüm oyun durumu ve deterministik oyun mantığı.
- `render/` — wgpu tabanlı render motoru.
- `app/` — pencere, girdi, yükleme ve kaydetme; sim, render, ui, audio ve net'i birbirine bağlar.
- `ui/` ve `sidebar/` — menüler, iletişim kutuları, oyun içi ekranlar ve yan panel.
- `audio/` — ses efektleri ve müzik.
- `net/` — deterministik lockstep. Henüz ağ aktarım katmanı yok.
- `rules/` — orijinal oyunun INI dosyaları ve katmanları.
- `map/` — haritalar, ortamlar (theaters), arazi, tetikleyiciler ve rastgele harita oluşturucu.
- `assets/` — orijinal dosya biçimleri için ayrıştırıcılar (`.mix`, `.shp`, `.vxl`, `.pal`, Bink videoları ve daha fazlası).
- `util/`, `asset_tools/` ve `bin/` — ortak yardımcılar, varlık inceleme araçları ve ek programlar.

## Parçalar nasıl birleşiyor

Değişebilen tüm oyun durumu tek bir yapıda tutulur: `Simulation` (`src/sim/world/mod.rs`). `SimRuntime::advance_frame()` bir kareyi orijinal oyunun kare sırasıyla çalıştırır.

Uygulama, oyuncu girdisini simülasyon için komutlara dönüştürür. Her kareden sonra yeni durumu render motoruna verir ve o karenin ürettiği sesleri çalar. Render işlemi yalnızca simülasyon durumunu okur; `sim/` ise hiçbir zaman `render/`, `ui/`, `sidebar/`, `audio/` veya `net/` modüllerine bağımlı değildir.

## Determinizm

Aynı durum, girdiler ve rastgele tohum her platformda ve her işlemcide aynı sonucu vermelidir.

- Simülasyon matematiği, kayan noktalı sayılar yerine sabit noktalı `SimFixed` türünü tercih eder.
- Üç rastgele sayı akışı, orijinaldeki üç üretece karşılık gelir. Yanlış akıştan yapılan bir çekiliş, sonraki tüm çekilişleri değiştirir.
- Nesneler, orijinaldeki aktif nesne sırasına göre sırayla hareket eder.

Her kare, lockstep çok oyunculu modun dayanacağı bir durum hash'iyle biter.
