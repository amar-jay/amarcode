# Amarcode Staj Raporları — Toplu Kaynakça

Bu kaynakça, 18 günlük staj raporunda başvurulan proje dosyalarını, oluşturulan teknik diyagramları, resmî dokümantasyonu ve teknik standartları tek bir yerde toplamaktadır. İnternet kaynakları için son erişim tarihi **7 Eylül 2026**’dır.

## 1. Amarcode Projesi ve Genel Belgeler

1. Amarcode. (2026). *Amarcode GitHub deposu*. GitHub. https://github.com/amar-jay/amarcode
2. Amarcode. (2026). *Amarcode proje açıklaması ve README dosyası*. GitHub. https://github.com/amar-jay/amarcode/blob/main/README.md
3. Amarcode. (2026). *Amarcode daemon: Mimari ve çalışma kuralları*. GitHub. https://github.com/amar-jay/amarcode/blob/main/crates/daemon/README.md
4. Amarcode. (2026). *Amarcode ACP adaptörü*. GitHub. https://github.com/amar-jay/amarcode/blob/main/crates/amarcode-acp/README.md
5. Amarcode. (2026). *Daemon Registry açıklaması*. GitHub. https://github.com/amar-jay/amarcode/blob/main/crates/daemon-registry/README.md
6. Amarcode. (2026). *Tauri kaldırma ve veri temizleme notları*. GitHub. https://github.com/amar-jay/amarcode/blob/main/crates/application/src-tauri/UNINSTALL.md

## 2. Kullanıcı Arayüzü ve Ön Yüz Kaynak Kodları

7. Amarcode. (2026). *Ana React uygulama bileşeni (`App.tsx`)*. GitHub. https://github.com/amar-jay/amarcode/blob/main/crates/application/src/App.tsx
8. Amarcode. (2026). *Ana istem giriş bileşeni (`main-prompt-input.tsx`)*. GitHub. https://github.com/amar-jay/amarcode/blob/main/crates/application/src/components/main-prompt-input.tsx
9. Amarcode. (2026). *Canlı sohbet ekranı (`live-chat-screen.tsx`)*. GitHub. https://github.com/amar-jay/amarcode/blob/main/crates/application/src/components/live-chat-screen.tsx
10. Amarcode. (2026). *Kod bloğu bileşeni (`code-block.tsx`)*. GitHub. https://github.com/amar-jay/amarcode/blob/main/crates/application/src/components/ai-elements/code-block.tsx
11. Amarcode. (2026). *Diff kartı bileşeni (`diff-artifact-card.tsx`)*. GitHub. https://github.com/amar-jay/amarcode/blob/main/crates/application/src/components/diff-artifact-card.tsx
12. Amarcode. (2026). *Çalışma alanı dosya ağacı (`workspace-file-tree.tsx`)*. GitHub. https://github.com/amar-jay/amarcode/blob/main/crates/application/src/components/workspace-file-tree.tsx
13. Amarcode. (2026). *Çalışma alanı diff görüntüleyicisi (`workspace-diff-viewer.tsx`)*. GitHub. https://github.com/amar-jay/amarcode/blob/main/crates/application/src/components/workspace-diff-viewer.tsx
14. Amarcode. (2026). *Bekleyen ajan isteği bileşeni (`pending-agent-request.tsx`)*. GitHub. https://github.com/amar-jay/amarcode/blob/main/crates/application/src/components/pending-agent-request.tsx
15. Amarcode. (2026). *Oturum yapılandırma kontrolleri (`session-config-controls.tsx`)*. GitHub. https://github.com/amar-jay/amarcode/blob/main/crates/application/src/components/session-config-controls.tsx
16. Amarcode. (2026). *Daemon olaylarını işleyen React hook’u (`use-daemon-events.ts`)*. GitHub. https://github.com/amar-jay/amarcode/blob/main/crates/application/src/hooks/use-daemon-events.ts
17. Amarcode. (2026). *Sohbet durum modülü (`chats.ts`)*. GitHub. https://github.com/amar-jay/amarcode/blob/main/crates/application/src/state/chats.ts
18. Amarcode. (2026). *Canlı sohbet durum modülü (`live-chat.ts`)*. GitHub. https://github.com/amar-jay/amarcode/blob/main/crates/application/src/state/live-chat.ts
19. Amarcode. (2026). *İzin modu durum modülü (`permission-mode.ts`)*. GitHub. https://github.com/amar-jay/amarcode/blob/main/crates/application/src/state/permission-mode.ts
20. Amarcode. (2026). *Oturum yapılandırma durum modülü (`session-config.ts`)*. GitHub. https://github.com/amar-jay/amarcode/blob/main/crates/application/src/state/session-config.ts
21. Amarcode. (2026). *Daemon olayları ön yüz testleri (`daemon-events.test.ts`)*. GitHub. https://github.com/amar-jay/amarcode/blob/main/crates/application/src/state/daemon-events.test.ts
22. Amarcode. (2026). *Rust türlerinden üretilmiş TypeScript protokolü (`protocol.ts`)*. GitHub. https://github.com/amar-jay/amarcode/blob/main/crates/application/src/generated/protocol.ts

## 3. Tauri Masaüstü Katmanı ve Daemon Yönetimi

23. Amarcode. (2026). *Tauri–daemon köprü modülü (`bridge.rs`)*. GitHub. https://github.com/amar-jay/amarcode/blob/main/crates/application/src-tauri/src/daemon/bridge.rs
24. Amarcode. (2026). *Tauri daemon yöneticisi (`manager.rs`)*. GitHub. https://github.com/amar-jay/amarcode/blob/main/crates/application/src-tauri/src/daemon/manager.rs
25. Amarcode. (2026). *Daemon sürüm, indirme ve güncelleme modülü (`release.rs`)*. GitHub. https://github.com/amar-jay/amarcode/blob/main/crates/application/src-tauri/src/daemon/release.rs
26. Amarcode. (2026). *Windows NSIS kaldırma kancası (`installer-hooks.nsh`)*. GitHub. https://github.com/amar-jay/amarcode/blob/main/crates/application/src-tauri/windows/installer-hooks.nsh

## 4. Daemon, Kalıcı Depolama ve Ortak Protokol Kaynak Kodları

27. Amarcode. (2026). *Daemon uygulama başlangıcı (`app.rs`)*. GitHub. https://github.com/amar-jay/amarcode/blob/main/crates/daemon/src/app.rs
28. Amarcode. (2026). *RPC bağlantı katmanı (`connection.rs`)*. GitHub. https://github.com/amar-jay/amarcode/blob/main/crates/daemon/src/rpc/connection.rs
29. Amarcode. (2026). *Daemon oturum yöneticisi (`session/manager.rs`)*. GitHub. https://github.com/amar-jay/amarcode/blob/main/crates/daemon/src/service/session/manager.rs
30. Amarcode. (2026). *Daemon gelen ACP olaylarını işleme modülü (`session/inbound.rs`)*. GitHub. https://github.com/amar-jay/amarcode/blob/main/crates/daemon/src/service/session/inbound.rs
31. Amarcode. (2026). *Daemon saklama katmanı (`store/mod.rs`)*. GitHub. https://github.com/amar-jay/amarcode/blob/main/crates/daemon/src/store/mod.rs
32. Amarcode. (2026). *Daemon sohbet saklama modülü (`store/chats.rs`)*. GitHub. https://github.com/amar-jay/amarcode/blob/main/crates/daemon/src/store/chats.rs
33. Amarcode. (2026). *Daemon servis kontrol modülü (`service_control.rs`)*. GitHub. https://github.com/amar-jay/amarcode/blob/main/crates/daemon/src/service_control.rs
34. Amarcode. (2026). *Daemon tek örnek kilidi (`instance_lock.rs`)*. GitHub. https://github.com/amar-jay/amarcode/blob/main/crates/daemon/src/instance_lock.rs
35. Amarcode. (2026). *ACP istemci uygulaması (`acp/client.rs`)*. GitHub. https://github.com/amar-jay/amarcode/blob/main/crates/daemon/src/acp/client.rs
36. Amarcode. (2026). *ACP ajan kayıt entegrasyonu (`registry.rs`)*. GitHub. https://github.com/amar-jay/amarcode/blob/main/crates/daemon/src/registry.rs
37. Amarcode. (2026). *İlk SQLite şema migration dosyası (`0001_initial.sql`)*. GitHub. https://github.com/amar-jay/amarcode/blob/main/crates/daemon/migrations/0001_initial.sql
38. Amarcode. (2026). *Oturum yapılandırma migration dosyası (`0003_chat_session_config.sql`)*. GitHub. https://github.com/amar-jay/amarcode/blob/main/crates/daemon/migrations/0003_chat_session_config.sql
39. Amarcode. (2026). *Ortak protokol crate’i (`protocol/src/lib.rs`)*. GitHub. https://github.com/amar-jay/amarcode/blob/main/crates/protocol/src/lib.rs
40. Amarcode. (2026). *Ortak RPC türleri (`protocol/src/rpc.rs`)*. GitHub. https://github.com/amar-jay/amarcode/blob/main/crates/protocol/src/rpc.rs
41. Amarcode. (2026). *Ortak olay türleri (`protocol/src/events.rs`)*. GitHub. https://github.com/amar-jay/amarcode/blob/main/crates/protocol/src/events.rs
42. Amarcode. (2026). *Daemon dikey dilim entegrasyon testleri (`vertical_slice.rs`)*. GitHub. https://github.com/amar-jay/amarcode/blob/main/crates/daemon/tests/vertical_slice.rs

## 5. ACP Adaptörü, Araçlar ve Testler

43. Amarcode. (2026). *ACP araç uygulaması (`tools.rs`)*. GitHub. https://github.com/amar-jay/amarcode/blob/main/crates/amarcode-acp/src/tools.rs
44. Amarcode. (2026). *ACP adaptörü çalışma zamanı (`runtime.rs`)*. GitHub. https://github.com/amar-jay/amarcode/blob/main/crates/amarcode-acp/src/runtime.rs
45. Amarcode. (2026). *ACP protokol transcript testleri (`protocol_transcript.rs`)*. GitHub. https://github.com/amar-jay/amarcode/blob/main/crates/amarcode-acp/tests/protocol_transcript.rs

## 6. Proje İçin Hazırlanan Teknik Diyagramlar

46. Amarcode. (2026). *Amarcode sistem mimarisi — Mermaid kaynak dosyası*. GitHub. https://github.com/amar-jay/amarcode/blob/main/assets/diagrams/system-architecture.mmd
47. Amarcode. (2026). *İstem yaşam döngüsü sıralama diyagramı — Mermaid kaynak dosyası*. GitHub. https://github.com/amar-jay/amarcode/blob/main/assets/diagrams/prompt-lifecycle-sequence.mmd
48. Amarcode. (2026). *Önce kaydet olay akışı — Mermaid kaynak dosyası*. GitHub. https://github.com/amar-jay/amarcode/blob/main/assets/diagrams/store-first-event-flow.mmd
49. Amarcode. (2026). *Daemon yaşam döngüsü durum diyagramı — Mermaid kaynak dosyası*. GitHub. https://github.com/amar-jay/amarcode/blob/main/assets/diagrams/daemon-lifecycle-state.mmd
50. Amarcode. (2026). *Daemon güncelleme ve geri alma sıralaması — Mermaid kaynak dosyası*. GitHub. https://github.com/amar-jay/amarcode/blob/main/assets/diagrams/daemon-update-rollback-sequence.mmd
51. Amarcode. (2026). *ACP araç izni ve terminal sıralaması — Mermaid kaynak dosyası*. GitHub. https://github.com/amar-jay/amarcode/blob/main/assets/diagrams/acp-tool-permission-sequence.mmd
52. Amarcode. (2026). *ACP ajan kayıt ve kurulum akışı — Mermaid kaynak dosyası*. GitHub. https://github.com/amar-jay/amarcode/blob/main/assets/diagrams/agent-registry-install-flow.mmd
53. Amarcode. (2026). *Oturum yapılandırma eşitleme akışı — Mermaid kaynak dosyası*. GitHub. https://github.com/amar-jay/amarcode/blob/main/assets/diagrams/session-config-sync.mmd

## 7. Resmî Teknoloji Dokümantasyonu

54. Bun. (2026). *Bun documentation*. https://bun.sh/docs
55. Cloudflare. (2026). *Cloudflare Workers documentation*. https://developers.cloudflare.com/workers/
56. Facebook Open Source. (2026). *React documentation*. https://react.dev/
57. Facebook Open Source. (2026). *Managing state*. React Documentation. https://react.dev/learn/managing-state
58. Facebook Open Source. (2026). *Rendering lists*. React Documentation. https://react.dev/learn/rendering-lists
59. Git Project. (2026). *Git documentation*. https://git-scm.com/docs
60. GitHub. (2026). *GitHub Actions documentation*. https://docs.github.com/actions
61. Jotai. (2026). *Jotai documentation*. https://jotai.org/
62. Microsoft. (2026). *TypeScript documentation*. https://www.typescriptlang.org/docs/
63. Rust Project Developers. (2026). *The Rust programming language*. https://www.rust-lang.org/
64. Rust Project Developers. (2026). *`std::path::Path` API documentation*. https://doc.rust-lang.org/std/path/struct.Path.html
65. Rust Project Developers. (2026). *`std::process::Command` API documentation*. https://doc.rust-lang.org/std/process/struct.Command.html
66. Serde Project. (2026). *Serde documentation*. https://serde.rs/
67. Tauri Programme. (2026). *Tauri documentation*. https://tauri.app/
68. Tauri Programme. (2026). *Calling Rust from the frontend*. Tauri Documentation. https://tauri.app/develop/calling-rust/
69. Tokio Project. (2026). *Tokio documentation*. https://tokio.rs/
70. Tokio Project. (2026). *Shared state*. Tokio Tutorial. https://tokio.rs/tokio/tutorial/shared-state

## 8. Protokoller, Standartlar ve Güvenlik Kaynakları

71. Agent Client Protocol. (2026). *Agent Client Protocol documentation*. https://agentclientprotocol.com/
72. CommonMark. (2026). *CommonMark specification*. https://spec.commonmark.org/
73. JSON-RPC Working Group. (2010). *JSON-RPC 2.0 specification*. https://www.jsonrpc.org/specification
74. OWASP Foundation. (2026). *Least privilege principle*. https://owasp.org/www-community/controls/Least_Privilege_Principle
75. OWASP Foundation. (2026). *Path traversal*. https://owasp.org/www-community/attacks/Path_Traversal
76. Semantic Versioning. (2026). *Semantic Versioning 2.0.0*. https://semver.org/
77. systemd. (2026). *systemd.service manual*. https://www.freedesktop.org/software/systemd/man/latest/systemd.service.html

## 9. SQLite Dokümantasyonu

78. SQLite. (2026). *Foreign key support*. https://www.sqlite.org/foreignkeys.html
79. SQLite. (2026). *Transactions*. https://www.sqlite.org/lang_transaction.html
80. SQLite. (2026). *Write-ahead logging*. https://www.sqlite.org/wal.html

## 10. Diyagram Üretim Aracı

81. Mermaid Project. (2026). *Mermaid documentation*. https://mermaid.js.org/
82. Mermaid Project. (2026). *Mermaid CLI*. GitHub. https://github.com/mermaid-js/mermaid-cli

## 11. Staj Kurumu Kaynakları

83. IQVizyon Dijital Dönüşüm A.Ş. (2026). *IQVizyon endüstriyel zekâ ve dijital dönüşüm platformu*. https://iqvizyon.com/
84. IQVizyon Dijital Dönüşüm A.Ş. (2026). *IQVizyon şirket profili*. LinkedIn. https://tr.linkedin.com/company/i%CC%87qvizyon
85. OSTİM Savunma ve Havacılık Kümelenmesi. (2026). *IQVizyon Dijital Dönüşüm A.Ş. firma profili*. https://www.ostimsavunma.org/firmalar/iqvizyon-dijital-donusum-as
86. IQVizyon Dijital Dönüşüm A.Ş. (2026). *Ürünler, hizmet alanları ve kurumsal bilgiler: Sıkça sorulan sorular*. https://www.iqvizyon.com/22-sss
