# Rosea

Детерминированный симулятор игровой логики osu!stable с собственным программным 2D-растеризатором для визуализации реплеев.

Пока есть workspace и чтение заголовка `.osu` через `parse::osu_header`.
Поддерживается v14, UTF-8 с BOM, LF/CRLF и пустые строки перед заголовком.
`parse::osu_mode` читает поле Mode из `[General]`: допускается только 0,
при отсутствии поля используется 0. Неверное значение и повтор поля — ошибки.
`parse::osu_difficulty` возвращает `Difficulty { cs, od, hp }` из полей
CircleSize, OverallDifficulty и HPDrainRate секции `[Difficulty]`.
Все три поля обязательны, должны быть конечными числами в диапазоне [0, 10]
и не должны повторяться. При неверном значении ошибка содержит имя поля и номер строки.
Остальные поля и объекты карты ещё не разбираются. Бинарь выводит название профиля.

Проверки:

```sh
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace --all-features
```
