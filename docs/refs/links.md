# ОФИЦИАЛЬНАЯ ДОКУМЕНТАЦИЯ OSU!WIKI


1. Спецификация формата карт (.osu file format v14):
https://osu.ppy.sh/wiki/en/Client/File_formats/osu_%28file_format%29

2. Спецификация формата реплеев (.osr file format):
https://osu.ppy.sh/wiki/en/Client/File_formats/osr_%28file_format%29

3. Параметры сложности карты (CS, AR, OD, HP и влияние модов EZ, HR, DT, HT):
https://osu.ppy.sh/wiki/en/Beatmap/Difficulty
https://osu.ppy.sh/wiki/en/Beatmap/Circle_size
https://osu.ppy.sh/wiki/en/Beatmap/Approach_rate
https://osu.ppy.sh/wiki/en/Beatmap/Overall_difficulty

4. Модификаторы геймплея (битовые флаги и множители):
https://osu.ppy.sh/wiki/en/Gameplay/Game_modifier


ЭТАЛОННЫЕ ИСХОДНИКИ В РЕПОЗИТОРИИ OSU!LAZER (ppy/osu) И OSU-FRAMEWORK (ppy/osu-framework)
Важно: наш проект реализует поведение osu!stable (legacy), поэтому из репозитория osu!lazer берем логику Legacy-декодеров и мода Classic (OsuModClassic).

1. Декодирование бинарного реплея .osr и потока LZMA:
https://github.com/ppy/osu/blob/master/osu.Game/Scoring/Legacy/LegacyScoreDecoder.cs
https://github.com/ppy/osu/blob/master/osu.Game/IO/Legacy/SerializationReader.cs
(Писать свой LZMA-декодер очень непростая задача, поэтому в mvp используем готовую библиотеку lzma-rs)

2. Декодирование текстового файла карты .osu и расчет длительности/тиков слайдеров:
https://github.com/ppy/osu/blob/master/osu.Game/Beatmaps/Formats/LegacyBeatmapDecoder.cs

3. Геометрия кривых слайдеров (Bezier, Catmull, Perfect Circle / CircularArc) и обрезка по пиксельной длине (SliderPath):
https://github.com/ppy/osu-framework/blob/master/osu.Framework/Utils/PathApproximator.cs
https://github.com/ppy/osu/blob/master/osu.Game/Rulesets/Objects/SliderPath.cs

4. Генерация вложенных объектов слайдера (голова, тики, репиты, ленивый хвост - tail leniency -36ms):
https://github.com/ppy/osu/blob/master/osu.Game/Rulesets/Objects/SliderEventGenerator.cs
https://github.com/ppy/osu/blob/master/osu.Game.Rulesets.Osu/Objects/Slider.cs

5. Расчет радиуса кружка (CS), времени появления (AR / Preempt / FadeIn) и legacy-стекинга нот (Stack Leniency):
https://github.com/ppy/osu/blob/master/osu.Game.Rulesets.Osu/Objects/OsuHitObject.cs
https://github.com/ppy/osu/blob/master/osu.Game.Rulesets.Osu/Beatmaps/OsuBeatmapProcessor.cs

6. Окна попаданий (300 / 100 / 50 / Miss) по параметру OD:
https://github.com/ppy/osu/blob/master/osu.Game.Rulesets.Osu/Scoring/OsuHitWindows.cs

7. Поведение osu!stable в osu!lazer (Note Lock, отсутствие привязки результата слайдера к времени нажатия на голову):
https://github.com/ppy/osu/blob/master/osu.Game.Rulesets.Osu/Mods/OsuModClassic.cs
https://github.com/ppy/osu/blob/master/osu.Game.Rulesets.Osu/Mods/OsuModHardRock.cs


ДОПОЛНИТЕЛЬНЫЕ РЕФЕРЕНСЫ С ОТКРЫТЫМ КОДОМ (ДЛЯ СВЕРКИ EDGE-КЕЙСОВ OSU!STABLE)

1. danser-go (эталонный рендерер и симулятор реплеев osu!stable на Go):
https://github.com/Wieku/danser-go

2. rosu-pp (точная математика карт, кривых слайдеров и атрибутов сложности на Rust):
https://github.com/MaxOhn/rosu-pp

