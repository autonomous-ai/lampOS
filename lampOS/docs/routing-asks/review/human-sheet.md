# Human blind check (G3b): 20 cases

For one person on the lamp team. It takes about fifteen minutes.

1. Read `policy.md` and `capabilities.md`. Do not open `routing-asks-v3.json`, `changelog.md`, `validation.md` or anything else in `review/`.
2. For each ask below, walk the policy's **Order of application** and take the first rule that fits.
3. Copy `review/h-template.json` to `review/h.json` and fill one row per ask, matching the `#` below to `id`:
   - `expect`: `answer` or `hand_over`.
   - `doors`: for a hand-over only, every door the rule accepts (`ask`, `task`, `answer`); leave `[]` for an answer.
   - `acts`: for a P2 label only, the class (`light`, `timer`, `reminder`, `volume`, `memory`, `time`, `weather`, `search`, `skill`); leave `""` otherwise.
   - `rule`: `P1`–`P8` as in the policy.
   - `confidence`: `high`, `medium` or `low`.
4. Score it from `docs/routing-asks/`:
   `python3 -I tools/rasks.py agree routing-asks-v3.json review/h.json --key review/key-human.json --partial`

Empty `acts` and empty `doors` on an answer row are fine; a `hand_over` row without doors is rejected with its id.

A label that differs from v3 is not an error on your side. It means the rule may be unclear, and the rule is reviewed first.

| # | ask |
|---|---|
| 1 | Make me 20 flashcards for my biology chapter on cells and save them in a file. |
| 2 | Make a weekly meal plan for a family of four with a shopping list. |
| 3 | Bảy nhân tám bằng mấy? |
| 4 | Research the best way to teach a seven-year-old multiplication and give me a plan. |
| 5 | For a soft egg, how many minutes in the boiling water? |
| 6 | Viết bài phát biểu khoảng 3 phút cho buổi họp lớp cuối năm. |
| 7 | Compare iPhone 15 and Samsung S24 prices across a few online shops. |
| 8 | Dim the light, I'm just watching a video now. |
| 9 | Set a timer for 25 minutes, I'm starting homework. |
| 10 | Order a pizza for dinner tonight. |
| 11 | Make the light a bit warmer, it's getting late. |
| 12 | Do I need a jacket tomorrow morning? |
| 13 | Lên kế hoạch ôn thi tiếng Anh trong một tháng cho mình. |
| 14 | Viết cho mình một chương trình nhỏ đoán số để mình học lập trình. |
| 15 | Put together a reading list of ten good books for a twelve-year-old, with a line on each. |
| 16 | What's 15 percent of 240? |
| 17 | Mình buồn quá, nói chuyện với mình chút đi. |
| 18 | Say goodnight to my daughter, she's going to bed. |
| 19 | Tìm giúp mình vài trường đại học có ngành khoa học máy tính ở Hà Nội và so sánh học phí nhé. |
| 20 | Tìm mấy công thức làm bánh flan không cần lò, so sánh xem cách nào dễ nhất. |
