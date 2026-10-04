# 마도 사오륙 (PC-98) 한글 패처

`Disc Station Vol. 09`에 수록된 PC-98용 《마도 사오륙》에 한글 패치를 적용하는 Rust 코드입니다. CD 이미지와 FAT16 HDI 판별·재조립, Compile-LZ 해제·재압축, 메시지 파일 재삽입과 버퍼 확장, 조사 처리, 한글 글리프 생성과 V30 렌더러 훅, 그래픽 문구 재구성을 제공합니다.

배포용 패치와 적용 방법은 [마도물어 시리즈 한글 번역 프로젝트](https://github.com/mcpads/madou-monogatari-kr-patch/tree/main/pc98-madou-456)에서 제공합니다.

## 제공하지 않는 것

이 저장소에는 원본 CD·HDI, 패치를 적용한 이미지, 번역 JSON, 폰트 파일이 없습니다. 따라서 이 저장소만으로는 배포 패치를 다시 만들 수 없습니다. 아래 입력을 직접 갖춘 경우에만 `build-localized`와 패치용 게임 파일 생성이 진행됩니다.

이 커밋의 코드는 배포 저장소의 1.0.0 패치를 만든 코드와 같습니다. 아래 입력을 갖추고 [디스크와 패치 생성](#디스크와-패치-생성) 절차를 RetroGame Patcher `31d2a8f`로 실행하면 배포된 1.0.0 ZIP과 바이트 단위로 같은 패키지가 만들어집니다.

## 빌드와 테스트

```bash
cargo build --release
cargo test
cargo test --features analysis
```

기본 테스트는 합성 입력만 사용합니다. 폰트나 번역 JSON이 필요한 테스트는 `#[ignore = "requires ..."]`로 필요한 입력을 밝혀 두었습니다. 입력을 갖춘 뒤 `cargo test --all-features -- --ignored`로 실행하며, 입력이 없으면 성공으로 넘어가지 않고 실패합니다.

기본 바이너리는 `verify-sources`, `build`, `build-localized`만 제공합니다. 게임 파일 추출, 패치용 게임 파일 생성과 정적 분석 명령은 `analysis` feature의 `pc98-madou456-analysis` 바이너리에 있습니다.

## 지원 원본

두 입력 모두 크기와 SHA-256이 일치해야 하며, 다르면 빌더가 진행하지 않습니다. 배포 패치를 적용할 때는 CD 이미지만 필요합니다.

| 원본 | 크기 | SHA-256 |
| --- | --- | --- |
| Disc Station Vol. 09 CD 이미지 (raw Mode 1/2352) | 46,901,232바이트 | `a288832c3f1ff2ff457f457d2db450eb27d8bb24f902e1dd886322aeb097c7db` |
| Disc Station Vol. 09-10-11 설치 HDI | 41,568,256바이트 | `8808a0da11959721a588d07e6c9973a8707ee01f0849d8a3e9d5a1e7e7caef2c` |

CD의 `DS9_DATA/MADOU456` 게임 파일 176개를 쓰고, 설치 HDI에서는 부팅용 DOS 시스템을 가져옵니다.

```bash
cargo run --release -- verify-sources \
  --source-cd "Disc Station Vol. 09.img" \
  --system-hdi "Disc Station Vol. 09-10-11 [installed].hdi"
```

## 빌드 입력

입력은 `assets/` 아래에 두며, `MADOU456_ASSET_DIR` 환경 변수로 다른 디렉터리를 지정할 수 있습니다. 번역 카탈로그 경로는 `--translation-catalog`로 따로 받습니다.

| 입력 | 기본 경로 | 비고 |
| --- | --- | --- |
| 번역 카탈로그 | `assets/translations/catalog.json` | 카탈로그가 가리키는 `contexts.json`과 `batches/*.json` 포함 |
| 그래픽 문구 번역 | `assets/translations/graphic-text.json` | 타이틀·설정·선택 화면의 그래픽 문구 |
| `MADO456.COM` UI 번역 | `assets/translations/mado456-ui-ko.json` | |
| 본문 폰트 | `assets/fonts/Galmuri14.ttf` | [Galmuri](https://github.com/quiple/galmuri) 2.404 |
| 설정 화면 문구 폰트 | `assets/fonts/NeoDunggeunmo.ttf` | [Neo둥근모](https://github.com/neodgm/neodgm) 1.600 |
| 선택 화면 안내 폰트 | `assets/fonts/BMJUA.ttf` | [배민 주아체](https://www.woowahan.com/fonts) |
| 폰트 라이선스 | `assets/fonts/Galmuri-OFL.txt`, `NeoDunggeunmo-OFL.txt`, `BMJUA-OFL.txt` | 각 폰트 배포처의 OFL 전문 |

폰트는 재배포 조건을 이 저장소에서 보장할 수 없어 포함하지 않습니다. 각 폰트의 라이선스는 배포처에서 확인하세요. `assets/fonts/*.json`의 폰트 프로필이 크기·기준선과 폰트 SHA-256을 고정하며, 폰트 파일의 SHA-256이 다르면 빌드가 진행하지 않습니다.

```text
6fe6c3fe4369e3837ac348431e8670733d67aa4bd550982baa72cc93c81a1c68  Galmuri14.ttf
d61b60eccb731f8ca9c7da582e4a05a94db66b570471809950aa9a7261b941d6  NeoDunggeunmo.ttf
e8e6aa8b1b662c7bf0d7f136f29e822e0985176458a6e5d0ba08afc4a5c901a9  BMJUA.ttf
```

## 디스크와 패치 생성

독립 실행 한글 HDI는 다음처럼 만듭니다. 번역하지 않은 HDI는 `build`로 만듭니다. 모든 명령은 입력을 바꾸지 않고 기존 출력도 덮어쓰지 않습니다.

```bash
cargo run --release -- build-localized \
  --source-cd "Disc Station Vol. 09.img" \
  --system-hdi "Disc Station Vol. 09-10-11 [installed].hdi" \
  --translation-catalog assets/translations/catalog.json \
  --output out/madou456-ko.hdi
```

배포 패치는 [RetroGame Patcher](https://github.com/mcpads/retro-patcher)의 ISO9660 디렉터리 패키지(`recipe.json`과 파일별 BPS를 담은 ZIP)입니다. 원본 게임 파일 176개에 한글화로 바뀐 파일을 덮어쓴 디렉터리를 만들고, `packaging/iso-directory-plan.json`으로 패키지를 작성합니다.

```bash
cargo run --release --features analysis --bin pc98-madou456-analysis -- extract-game-files \
  --source-cd "Disc Station Vol. 09.img" \
  --system-hdi "Disc Station Vol. 09-10-11 [installed].hdi" \
  --output-dir out/content
cargo run --release --features analysis --bin pc98-madou456-analysis -- rebuild-localized-game-files \
  --source-cd "Disc Station Vol. 09.img" \
  --system-hdi "Disc Station Vol. 09-10-11 [installed].hdi" \
  --translation-catalog assets/translations/catalog.json \
  --output-dir out/localized
find out/localized -type f ! -name manifest.json -exec cp {} out/content/ \;

# RetroGame Patcher 저장소의 patch-core에서
cargo run --release --bin retro-patch-author -- create-iso-directory \
  path/to/iso-directory-plan.json "Disc Station Vol. 09.img" path/to/out/content madou456-ko.zip
```

`rebuild-localized-game-files`는 바뀐 게임 파일과 `manifest.json`만 씁니다. 패키지에는 원본 CD나 완성 ISO가 들어가지 않습니다.

## 라이선스

이 저장소의 소스 코드는 [MIT License](LICENSE)로 제공합니다.
