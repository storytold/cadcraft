# CADCraft — что нового / What's new

Изменения после версии 0.9.0. Новые записи добавляются сверху, с датой.
Пожалуйста, попробуйте описанное и напишите, что работает не так, чего не хватает и какие у вас есть идеи.

Changes since version 0.9.0. New entries go on top, with a date.
Please try what is described here and tell us what doesn't work as expected, what is missing, and what ideas you have.

---

## 24.10.2026 (0.9.0-b17)

### Русский

**Штриховка: «Добавить: Указать точки»**
- Контур вокруг указанной точки теперь находится и в больших чертежах. Раньше в поиске участвовали все 17 тысяч объектов чертежа, и в «אריה צוק» контур часто не находился. Теперь, как в AutoCAD, контур ищется среди объектов на экране и рядом с ним.
- Текст, размеры и рамки вокруг блоков и выносок больше не мешают. Раньше прямоугольник вокруг текста выноски или вокруг блока мог разрезать комнату или ящик пополам. Теперь блоки и выноски ограничивают штриховку только своими линиями.
- Если область вокруг точки не замкнута, появляется окно «Штриховка — ошибка определения контура» с объяснением, и ничего не создаётся.
- Если найденный контур уходит далеко за пределы экрана (обычно это значит, что в границе есть разрыв и область «вытекла» наружу), появляется такое же окно с предупреждением. Если область действительно большая, уменьшите масштаб, чтобы она была видна целиком, и укажите точку ещё раз.

**Штриховка: «Добавить: Выбрать объекты»**
- Выбранные объекты подсвечиваются, пока выбор не закончен.
- Теперь подходят не только замкнутые полилинии и окружности, но и отрезки, дуги и незамкнутые полилинии, которые вместе образуют замкнутую область. Их можно выбирать по одному: например, четыре отрезка, образующих прямоугольник. Выступающие концы на углах не мешают.
- Пока выбранные объекты ещё не замыкают область, в командной строке написано «не ограничивают область». Если нажать Enter, появится окно с предупреждением, а команда подождёт недостающие объекты.
- Если часть выбранных объектов не участвует в контуре, штриховка создаётся по остальным, и появляется предупреждение.

**Обрезка штриховки**
- Линии внутри блоков теперь тоже служат режущими кромками, для штриховки и для любых других объектов. Раньше блоки при обрезке не учитывались, поэтому в чертежах, где стенки и полки нарисованы блоками, «Обрезать» не находило пересечений и в быстром режиме удаляло штриховку целиком.
- Щелчок по краю штриховки обрезает часть, прилегающую к этому месту. Раньше такой щелчок удалял штриховку целиком.
- Штриховка удаляется целиком, только когда её действительно ничто не пересекает. Линия, которая заходит в штриховку и обрывается внутри, не делит её на части, как и в AutoCAD.

### English

**Hatch: Add: Pick points**
- The boundary around a picked point is now found in large drawings too. Before, all 17,000 objects of the drawing took part in the search, and in "אריה צוק" the boundary was often not found. Now, as in AutoCAD, the boundary is looked for among the objects on screen and near it.
- Text, dimensions, and the boxes around blocks and leaders no longer get in the way. Before, the box around a leader's text or around a block could cut a room or a drawer in two. Now blocks and leaders bound a hatch only with their own lines.
- If the area around the point is not closed, a "Hatch - Boundary Definition Error" window explains it, and nothing is created.
- If the boundary found reaches far outside the screen (usually a gap that let the area run out), the same window warns about it. If the area really is that large, zoom out to show all of it and pick again.

**Hatch: Add: Select objects**
- The selected objects are highlighted while you select.
- Not only closed polylines and circles work now, but also lines, arcs and open polylines that together form a closed area. You can pick them one by one, for example four lines forming a rectangle. Ends running past the corners don't matter.
- While the selected objects don't close an area yet, the command line says "not enclosing an area yet". Pressing Enter shows a warning window, and the command waits for the missing objects.
- If some of the selected objects take no part in the boundary, the hatch is made from the others, with a warning.

**Trimming hatches**
- Lines inside blocks now also act as cutting edges, for hatches and for any other object. Before, blocks were ignored by TRIM, so in drawings where walls and shelves are blocks, TRIM found no crossings and in Quick mode erased the whole hatch.
- Clicking the edge of a hatch trims the part next to that spot. Before, such a click erased the whole hatch.
- A hatch is erased entirely only when nothing crosses it. A line that enters a hatch and ends inside it does not split it, as in AutoCAD.

---

## 23.10.2026 (0.9.0-b16)

### Русский

**Окно редактирования текста**
- Когда открываете существующий текст, весь текст сразу выделен: можно сразу печатать новый.
- Отмена и возврат: кнопки со стрелками рядом с «ОК» и «Отмена», а также Ctrl+Z (отменить), Ctrl+Y и Ctrl+Shift+Z (вернуть). Текст, набранный без пауз, отменяется одним шагом, а не по одной букве.
- Над текстом курсор мыши становится текстовым (I-образным).

**Формулы в размерах, выносках и многострочном тексте**
- Формулы теперь работают и при редактировании: в многострочном тексте, в тексте размера и в тексте выноски, а не только при создании нового текста.
- Формула работает, только если весь текст — одна строка, начинающаяся с «=».
- В размере знак <> в формуле означает сам измеренный размер. Например, =<>/2 у размера 1250 покажет 625, а =ROUND(<>/1000;2) покажет метры. При изменении геометрии значение пересчитывается.

**Обрезать и Удлинить: подсветка того, что произойдёт**
- Во время команд «Обрезать» (TRIM) и «Удлинить» (EXTEND) наведите курсор на объект. Часть, которая будет удалена, светится красным. Часть, которая будет добавлена, светится голубым. Так видно заранее, что сделает щелчок.
- В быстром режиме объект, который ничто не пересекает, удаляется целиком. В этом случае светится весь объект.

**Обрезка штриховки**
- «Обрезать» теперь работает и со штриховкой. Линии, пересекающие штриховку, делят её на части. Щелчок по части удаляет её из штриховки, остальное остаётся. Как и в AutoCAD, после обрезки штриховка перестаёт быть ассоциативной.
- При наведении удаляемая часть штриховки подсвечивается красной заливкой.
- Дуги в контуре обрезанной штриховки заменяются короткими отрезками.

**Командная строка**
- Последние команды над командной строкой показываются только при наведении мыши на командную строку. Раньше они были видны всегда.

**Подсказки к инструментам**
- В подсказке у каждой кнопки инструмента теперь есть короткое описание того, что делает команда (на русском, английском и украинском).

**Диспетчер размерных стилей: правильный предпросмотр**
- Предпросмотр берёт из чертежа пользовательские стрелки (блоки), текстовые стили со шрифтами и типы линий. Раньше пользовательские стрелки (например, засечки в стилях «1-50», «1-100» чертежа «אריה צוק») в предпросмотре заменялись обычными стрелками.

### English

**Text editing window**
- Editing existing text opens it fully selected, so you can type over it right away.
- Undo and redo: arrow buttons next to OK and Cancel, plus Ctrl+Z (undo) and Ctrl+Y or Ctrl+Shift+Z (redo). Text typed without a pause is undone in one step, not letter by letter.
- The mouse pointer becomes a text (I-beam) cursor over the text.

**Formulas in dimensions, leaders and multiline text**
- Formulas now also work when editing multiline text, dimension text and leader text, not only when creating new text.
- A formula works only if the whole text is one line starting with "=".
- In a dimension, <> in a formula stands for the measured value. For example, =<>/2 on a 1250 dimension shows 625, and =ROUND(<>/1000;2) shows meters. The value updates when the geometry changes.

**Trim and Extend: see what will happen**
- During TRIM and EXTEND, hover over an object. The part that will be removed glows red. The part that will be added glows blue. You see the result before you click.
- In Quick mode an object that nothing crosses is erased entirely. In that case the whole object glows.

**Trimming hatches**
- TRIM now works on hatches. Lines crossing a hatch split it into parts. Clicking a part removes it from the hatch, and the rest stays. As in AutoCAD, the trimmed hatch is no longer associative.
- On hover, the part to be removed is highlighted with a red fill.
- Arcs in a trimmed hatch's boundary are replaced by short straight segments.

**Command line**
- Recent commands above the command line now appear only while the mouse is over the command line. Before, they were always visible.

**Tool tooltips**
- Each tool button's tooltip now includes a short description of what the command does (in Russian, English and Ukrainian).

**Dimension Style Manager: correct preview**
- The preview now uses the drawing's own user arrowheads (blocks), text styles with their fonts, and linetypes. Before, user arrowheads (for example, the ticks in the "1-50" and "1-100" styles of the "אריה צוק" drawing) were replaced with plain arrows in the preview.

---

## 23.10.2026 (0.9.0-b15)

### Русский

**Палитра «Свойства»: прокрутка длинных списков**
- Выпадающие списки в «Свойствах» (слои, типы линий, стили, выбор объектов по типу и другие), а также список «Загрузить…» типов линий и списки быстрых свойств теперь прокручиваются, если они слишком длинные. Полоса прокрутки видна всегда.

**Лист при первом открытии вписывается в экран**
- Когда вкладку листа открывают впервые, лист целиком вписывается в окно с небольшим отступом. Раньше для листов в миллиметрах внутри дюймового чертежа (как в «אריה צוק») масштаб получался в 25,4 раза неверным.

**Стартовая страница: недавние файлы списком или сеткой**
- Рядом с заголовком «Недавние» есть две кнопки: «Список» и «Сетка с эскизами».
- В сетке у каждого файла есть эскиз. Для DWG и DXF берётся эскиз, сохранённый в самом файле (AutoCAD его записывает). Если его нет, CADCraft рисует пространство модели сам. Эскизы маленькие (обычно 10–25 КБ) и хранятся в папке настроек программы. Когда файл меняется, эскиз создаётся заново.
- Если файла по прежнему пути нет, вместо эскиза показан крупный значок: пунктирный лист с красным кругом и вопросительным знаком, а под именем написано «Файл не найден». В режиме списка рядом с таким файлом стоит маленький такой же значок.

**Текст: выравнивание, «справа налево», «вверх ногами», рамка**
- В «Свойствах» у текста появились «Выравнивание» (все 15 вариантов AutoCAD: Влево, По центру, Вправо, Вписанный, Посередине, По ширине и девять вариантов «Вверху/Посередине/Внизу слева/по центру/справа»), «Справа налево (зеркально)» и «Вверх ногами».
- При смене выравнивания текст остаётся на месте: перемещается только точка, по которой он выровнен.
- У многострочного текста: «Выравнивание» (9 вариантов, текст тоже остаётся на месте), «Поворот» и «Рамка текста». Рамка рисуется вокруг текста и сохраняется так, что AutoCAD её тоже видит.
- Флаги «Справа налево» и «Вверх ногами» читаются из чертежей AutoCAD для каждого текста отдельно, как в AutoCAD. Раньше CADCraft брал их из текстового стиля. Из-за этого тексты в «зеркальных» стилях (в «אריה צוק» это, например, стиль T-NEW) могли показываться зеркально, хотя в AutoCAD они нормальные. Новый текст, как и в AutoCAD, получает эти флаги от своего стиля.
- Выравнивание существующих текстов читалось правильно и раньше: группы 72/73 и точка выравнивания.

**Окно редактирования текста: иврит и английский в одной строке**
- Убран «второй» текст другим шрифтом, который был виден при выделении.
- Окно редактирования теперь своё, а не стандартное: текст в нём расположен так же, как на чертеже. Каждый абзац получает направление по первой «сильной» букве. Абзац, начинающийся на иврите, читается справа налево, на английском — слева направо. Внутри абзаца английские слова, числа, пробелы и знаки становятся на свои места по правилам Unicode, как в Word, и больше не «перепрыгивают».
- Стрелки ← и → двигают курсор туда, куда они указывают на экране, в любом направлении текста. Ctrl+стрелки переходят по словам, Shift+стрелки выделяют, выделение подсвечивает именно выбранные буквы. Работают Home/End, Ctrl+A, копирование, вырезание, вставка, отмена (Ctrl+Z) и возврат (Ctrl+Y), выделение мышью и двойной щелчок по слову.

**Формулы в тексте (как в Excel)**
- Текст, который начинается со знака «=», показывает результат формулы: =2400-2*18, =ROUND(PI()*125^2/1000000;3), =TEXT(TODAY();"dd.mm.yyyy").
- Формула хранится вместе с текстом. При двойном щелчке открывается формула, а не результат. Если ввести обычный текст без «=», формула удаляется.
- Пока набирается имя функции, рядом с курсором появляется список подходящих функций с описанием. Стрелки выбирают, Tab или Enter вставляют, Esc закрывает.
- Доступные функции: ABS, ACOS, AND, ASIN, ATAN, AVERAGE, CONCAT, COS, DATE, DAY, DEGREES, EXP, FILENAME, FIXED, HOUR, IF, INT, LEFT, LEN, LN, LOG10, LOWER, MAX, MID, MIN, MINUTE, MOD, MONTH, NOT, NOW, OR, PI, POWER, RADIANS, RIGHT, ROUND, ROUNDDOWN, ROUNDUP, SHEET, SIN, SQRT, SUM, TAN, TEXT, TODAY, TRIM, UPPER, YEAR. Операции: + - * / ^ % & и сравнения. Аргументы разделяются запятой или точкой с запятой. Ошибки показываются как в Excel: #DIV/0!, #NAME?, #VALUE!.
- Описание всех функций: «Справка ▸ Формулы в тексте».
- Дата и время берутся из часов Windows и пересчитываются при каждом открытии чертежа. Пересчитать сразу: «Сервис ▸ Обновить поля» (команда UPDATEFIELD).
- Поля даты из AutoCAD тоже работают. На листе «4» в «אריה צוק» дата в правом нижнем углу — это поле AutoCAD «дата» в формате dd.MM.yyyy. Теперь CADCraft его понимает и при открытии показывает сегодняшнюю дату, как AutoCAD. Само поле сохраняется в файле, и в AutoCAD оно продолжит работать.
- Формулы CADCraft записываются в файл так, что AutoCAD увидит их результат как обычный текст.

### English

**Properties palette: long lists scroll**
- Drop-down lists in Properties (layers, linetypes, styles, select by type and others), as well as the linetype "Load…" list and the Quick Properties lists, now scroll when they are too long. The scrollbar is always visible.

**A layout fits the screen the first time it is shown**
- When a layout tab is opened for the first time, the whole sheet fits the window with a small margin. Before, layouts in millimetres inside an inch drawing (as in "אריה צוק") came out 25.4 times off.

**Start page: recent files as a list or a grid**
- Two buttons next to the "Recent" heading: "List" and "Grid with previews".
- In the grid, each file has a preview. For DWG and DXF it is the preview saved in the file itself (AutoCAD writes one). When a file has none, CADCraft draws its model space. Previews are small (usually 10–25 KB) and kept in the program's settings folder. A changed file gets a new preview.
- If a file is no longer at its path, a large icon shows instead of the preview: a dashed page with a red circle and a question mark, and "File not found" under its name. In the list, such a file has the same small icon next to it.

**Text: justification, backwards, upside down, frame**
- Text in Properties now has "Justify" (all 15 AutoCAD options: Left, Center, Right, Aligned, Middle, Fit, and the nine Top/Middle/Bottom left/center/right ones), "Backwards" and "Upside down".
- Changing the justification keeps the text in place: only the point it is justified by moves.
- Multiline text: "Justify" (9 options; the text also stays in place), "Rotation" and "Text frame". The frame is drawn around the text and saved so that AutoCAD shows it too.
- Backwards and Upside down are read from AutoCAD drawings for each text, as AutoCAD does. CADCraft used to take them from the text style, which could mirror texts in "backwards" styles (in "אריה צוק", style T-NEW for example) that AutoCAD shows normally. As in AutoCAD, new text takes these settings from its style.
- The justification of existing texts was already read correctly (groups 72/73 and the alignment point).

**The text editing box: Hebrew and English on one line**
- The "second" text in another font that showed up while selecting is gone.
- The editing box is now CADCraft's own, not the standard one: text in it is laid out the same way as on the drawing. Each paragraph takes its direction from its first strong letter. A paragraph that starts in Hebrew reads right to left, one that starts in English left to right. Within a paragraph, English words, numbers, spaces and punctuation fall into place by the Unicode rules, as in Word, and no longer jump around.
- ← and → move the caret where they point on screen, whatever the text direction. Ctrl+arrows move by words, Shift+arrows select, and a selection highlights exactly the selected letters. Home/End, Ctrl+A, copy, cut, paste, undo (Ctrl+Z) and redo (Ctrl+Y), mouse selection and double-clicking a word all work.

**Formulas in text (as in Excel)**
- A text that starts with "=" shows the result of a formula: =2400-2*18, =ROUND(PI()*125^2/1000000;3), =TEXT(TODAY();"dd.mm.yyyy").
- The formula stays with the text. Double-clicking opens the formula, not the result. Typing plain text without "=" removes the formula.
- While a function name is typed, a list of matching functions with descriptions appears next to the caret. The arrows choose, Tab or Enter inserts, Esc closes.
- Available functions: ABS, ACOS, AND, ASIN, ATAN, AVERAGE, CONCAT, COS, DATE, DAY, DEGREES, EXP, FILENAME, FIXED, HOUR, IF, INT, LEFT, LEN, LN, LOG10, LOWER, MAX, MID, MIN, MINUTE, MOD, MONTH, NOT, NOW, OR, PI, POWER, RADIANS, RIGHT, ROUND, ROUNDDOWN, ROUNDUP, SHEET, SIN, SQRT, SUM, TAN, TEXT, TODAY, TRIM, UPPER, YEAR. Operators: + - * / ^ % & and comparisons. Arguments are separated by commas or semicolons. Errors show as in Excel: #DIV/0!, #NAME?, #VALUE!.
- All functions are described in Help ▸ Text Formulas.
- Dates and times come from the Windows clock and are worked out again every time the drawing is opened. To do it at once: Tools ▸ Update Fields (the UPDATEFIELD command).
- AutoCAD date fields work too. On sheet "4" in "אריה צוק", the date in the bottom right corner is an AutoCAD "date" field in the dd.MM.yyyy format. CADCraft now understands it and shows today's date when the drawing is opened, as AutoCAD does. The field itself is kept in the file and keeps working in AutoCAD.
- CADCraft formulas are saved so that AutoCAD shows their result as ordinary text.

---

## 22.10.2026 (0.9.0-b14)

### Русский

**Текст без заливки при печати**
- Вероятная причина найдена. Шрифты TrueType заливаются по правилу «ненулевой обмотки»: если контуры буквы перекрываются, место перекрытия всё равно закрашивается. CADCraft заливал буквы по другому правилу («чёт-нечет»), и у букв из перекрывающихся контуров середина оставалась пустой — была видна только обводка. Так устроены многие шрифты, в том числе TrueType-версии штриховых шрифтов, которые AutoCAD ставит в Windows (romanc__.ttf, txt_____.ttf и т. п.). Теперь буквы заливаются так же, как их рисует Windows: на экране, в предварительном просмотре и при печати.
- Пожалуйста, проверьте лист «4» ещё раз. Если где-то текст всё ещё полый, пришлите снимок.

**Редактор текста показывает шрифт чертежа**
- В окне редактирования текста (TEXT, MTEXT, мультивыноски) буквы рисуются тем же шрифтом, что и на чертеже: SHX, TrueType или встроенным, с коэффициентом ширины и наклоном стиля. Курсор и выделение совпадают с буквами.
- Текст в окне стал немного крупнее.

**Прямоугольник: ширина и высота**
- После первого угла ПРЯМОУГОЛЬНИК (RECTANG) показывает два поля прямо у сторон: ширину под (или над) горизонтальной стороной и высоту рядом с вертикальной. Введите ширину, нажмите Tab, введите высоту, нажмите Enter.
- Прямоугольник строится в ту сторону от первого угла, где находится курсор.

**Окошко с вариантами**
- Окошко с вариантами (например, «Да/Нет» в ЗЕРКАЛО) теперь открывается ниже перекрестия и не закрывает курсор.

**Режимы рисования (DSETTINGS)**
- Окно стало ниже, его содержимое прокручивается, полоса прокрутки видна всегда.

**Тип и вес линий на панели «Слои»**
- Рядом со списком типов линий появился список весов линий (ПоСлою, ПоБлоку, По умолчанию, 0.00–2.11 мм). Выбранный вес становится текущим для новых объектов.
- В обоих списках слева от названия нарисован образец: штрихи типа линии или линия нужной толщины, отделённые от названия разделителем.

**Привязка курсора к ручкам**
- Раньше её не было. Теперь, когда никакая команда не выполняется, перекрестие «прилипает» к ручке выбранного объекта, на которую наведена мышь. Меню ручки появляется вдвое быстрее.
- Включается и выключается в «Режимах рисования» (DSETTINGS), флажок «Привязывать перекрестие к ручкам», или переменной GRIPSNAP (1/0). По умолчанию включена.

### English

**Text printing without fill**
- Likely cause found. TrueType fonts are filled by the non-zero winding rule: where a letter's contours overlap, the overlap is still ink. CADCraft filled letters by the even-odd rule instead, so letters made of overlapping contours came out hollow, showing only their outline. Many fonts are built that way, including the TrueType versions of stroke fonts that AutoCAD installs in Windows (romanc__.ttf, txt_____.ttf and so on). Letters are now filled the way Windows draws them: on screen, in Plot Preview and in plots.
- Please check sheet "4" again, and send a screenshot if any text is still hollow.

**The text editor shows the drawing's font**
- The text editing box (TEXT, MTEXT, multileaders) draws letters in the drawing's own font: SHX, TrueType or the built-in one, with the style's width factor and oblique angle. The caret and selection line up with the letters.
- Text in the box is a little larger.

**Rectangle: width and height**
- After the first corner, RECTANG shows two boxes right at the sides: the width under (or over) the horizontal side and the height beside the vertical side. Type the width, press Tab, type the height, press Enter.
- The rectangle goes from the first corner toward the side where the cursor is.

**Options box**
- The options box (for example Yes/No in MIRROR) now opens below the crosshair and no longer covers the cursor.

**Drafting Settings (DSETTINGS)**
- The window is shorter, its contents scroll, and the scrollbar is always visible.

**Linetype and lineweight on the Layers bar**
- A lineweight list (ByLayer, ByBlock, Default, 0.00–2.11 mm) is now beside the linetype list. The weight you pick becomes current for new objects.
- Both lists show a sample left of each name, behind a separator: the linetype's dashes or a line of that weight.

**Cursor snapping to grips**
- This didn't exist before. With no command running, the crosshair now locks onto the grip of a selected object under the mouse. The grip menu appears twice as fast.
- Turn it on or off in Drafting Settings (DSETTINGS) with "Snap the crosshair to grips", or with the GRIPSNAP variable (1/0). It is on by default.

---

## 22.10.2026 (0.9.0-b13)

### Русский

**Штриховка: цвет фона**
- У штриховки появилось свойство «Цвет фона»: в палитре «Свойства», в окне «Штриховка и градиент» и в команде HATCHEDIT. Можно выбрать «Нет», «ПоСлою», «ПоБлоку», стандартный цвет или любой другой.
- Цвет фона, заданный в AutoCAD, читается из файла и записывается обратно, так что AutoCAD его тоже увидит. У сплошной заливки фона нет.

**Иврит на чертеже вместо русского**
- Найдена причина. Шрифты SHX этого чертежа (romans, txt, simplex и др.) хранят буквы по кодам Unicode. Программа считала их «одноязычными» шрифтами, где иврит лежит на кодах 0xE0–0xFA, и брала оттуда знаки, а там в этих шрифтах русские буквы. Теперь иврит берётся из таких кодов только у настоящих ивритских шрифтов (кодировки 1255 и 862). В остальных шрифтах буквы ищутся по Unicode, а если буквы нет, используется встроенный шрифт CADCraft.
- Шрифты вроде GIL, в которых латинские буквы нарисованы как ивритские (ввод с английской раскладки), так и показываются — это задумано самим шрифтом.

**Спецсимволы**
- Знаки ⌀ (%%c), ° (%%d) и ± (%%p) показываются при любом стиле и шрифте. Если в шрифте SHX такого знака нет, он рисуется встроенным шрифтом. Раньше в некоторых ивритских шрифтах вместо ⌀ появлялась «]».

**Листы (Layout) чертежей AutoCAD**
- Найдена причина сдвига. Видовой экран AutoCAD смотрит не только в свою «центральную точку», но и со смещением на «цель вида», и программа это смещение не учитывала. Теперь учитывает: чертежи вернулись на листы на своё место (проверено на «אריה צוק»).

**Формулы в командной строке**
- Везде, где программа спрашивает расстояние, длину, радиус или число, можно ввести формулу, начинающуюся с «=»: =1200+350*2, =(2400-50)/3. Работают + − * /, скобки, минус перед числом, десятичная запятая. Результат показывается в истории: «=1200+350*2 = 1900».
- Пробел внутри формулы — это пробел, а не Enter.

**Выбор вариантов рядом с курсором**
- Когда команда спрашивает выбор между вариантами (например, ЗЕРКАЛО: «Удалить исходные объекты? [Да/Нет]»), рядом с курсором появляется окошко с вариантами. Вариант можно нажать мышью или выбрать стрелками ↑ ↓ и Enter. Вариант по умолчанию уже выделен. Можно и просто печатать, как раньше.

**Курсор**
- При панорамировании средней кнопкой мыши курсор становится значком «Перемещение».
- Над бумагой листа (Layout) перекрестие курсора становится чёрным.

**Пакетная печать**
- Пока окно открыто, остальная программа неактивна (затемнена).
- Несколько листов выделяются щелчком с Ctrl, диапазон — с Shift, рамкой мышью (начиная с пустого места под строками), все — Ctrl+A. Кнопки перемещения, удаления и копирования работают для всех выделенных листов сразу.
- Список разбит на столбцы «№», «Лист», «Чертёж», «Бумага» с разделителями. Названия обрезаются по ширине столбца и не наезжают друг на друга.

**Отменить и повторить**
- Новые значки в стиле Office 2010: толстая синяя дуга с крупной стрелкой.
- Рядом с кнопками «Отменить» и «Повторить» есть маленькая кнопка со стрелкой вниз. Она открывает список последних шагов (до 100, с прокруткой). При наведении подсвечиваются все шаги, которые будут отменены, а щелчок отменяет (или повторяет) их все сразу.

**Значок предварительного просмотра**
- Лупа отодвинута вправо и больше не закрывает принтер.

**Пока не воспроизведено**
- Текст, печатающийся контуром без заливки, у нас не получился: в PDF текст и размеры выходят залитыми. Пришлите, пожалуйста, снимок или файл, где это видно, и скажите, через что печатали (PDF, принтер) и чем смотрели.

### English

**Hatch: background colour**
- Hatches have a "Background color" property: in the Properties palette, the Hatch and Gradient dialog, and HATCHEDIT. Choose None, ByLayer, ByBlock, a standard colour, or any other colour.
- A background colour set in AutoCAD is read from the file and written back, so AutoCAD sees it too. Solid fills have no background.

**Hebrew turning into Russian on the drawing**
- Found the cause. This drawing's SHX fonts (romans, txt, simplex, …) store letters by their Unicode codes. CADCraft took them for single-language fonts that keep Hebrew at codes 0xE0–0xFA and drew the glyphs found there, which in these fonts are Russian letters. Now Hebrew is looked up at those codes only in real Hebrew fonts (code pages 1255 and 862). Other fonts are looked up by Unicode, and a missing letter is drawn with CADCraft's built-in font.
- Fonts such as GIL, which draw Latin letters as Hebrew ones (for typing on an English keyboard), keep showing that way, as the font intends.

**Special symbols**
- ⌀ (%%c), ° (%%d) and ± (%%p) show with every style and font. If an SHX font has no such sign, it is drawn with the built-in font. Some Hebrew fonts used to show "]" instead of ⌀.

**Layouts of AutoCAD drawings**
- Found the cause of the shift. An AutoCAD viewport looks at its view centre offset by its "view target", and CADCraft ignored that offset. It no longer does: the drawings are back in place on the sheets (checked with "אריה צוק").

**Formulas at the command line**
- Wherever a distance, length, radius or number is asked for, you can type a formula starting with "=": =1200+350*2, =(2400-50)/3. It takes + − * /, parentheses, a minus before a number and a decimal comma. The history shows the result: "=1200+350*2 = 1900".
- A space inside a formula is a space, not Enter.

**Choices next to the cursor**
- When a command asks you to choose between options (for example MIRROR: "Erase source objects? [Yes/No]"), a small box with the options appears next to the cursor. Click one, or move with ↑ ↓ and press Enter. The default option is already highlighted. Typing works as before.

**Cursor**
- While panning with the middle mouse button, the cursor shows the Move icon.
- Over the paper of a layout, the crosshair turns black.

**Batch Plot**
- While the window is open, the rest of the program is inactive (dimmed).
- Select several sheets with Ctrl+click, a range with Shift+click, a rectangle dragged with the mouse (start in the empty space below the rows), or all with Ctrl+A. Move, remove and duplicate act on all selected sheets.
- The list has columns "#", "Sheet", "Drawing" and "Paper" with separators. Names are cut to the column width and never overlap.

**Undo and Redo**
- New Office 2010-style icons: a thick blue arc with a big arrowhead.
- A small arrow button beside Undo and Redo opens a list of recent steps (up to 100, with a scrollbar). Hovering highlights all steps that will be undone, and a click undoes (or redoes) all of them at once.

**Plot Preview icon**
- The magnifier moved right and no longer covers the printer.

**Not reproduced yet**
- Text printing as an outline without fill didn't happen here: in PDF, text and dimensions come out filled. Please send a screenshot or a file where you see it, and say how you printed (PDF or printer) and what you viewed it with.

---

## 22.10.2026 (0.9.0-b12)

### Русский

**Свойства текста и выносок**
- В палитре «Свойства» у текста, многострочного текста и мультивыносок появилась строка «Стиль» (текстовый стиль). Она работает и для нескольких выбранных объектов сразу. Если стили разные, показывается «*РАЗНЫЕ*».
- В списках стрелок (у мультивыносок в «Свойствах» и в редакторе размерного стиля) рядом с каждой стрелкой нарисован её вид, в том числе у пользовательских стрелок-блоков.

**Ручка текста мультивыноски**
- У текста мультивыноски появилась своя ручка. Её меню: «Переместить с выноской», «Переместить только текст», «Восстановить положение текста».
- «Переместить с выноской» ставит текст в указанную точку, полочка встаёт рядом с текстом, а выноска тянется к ней от своей последней точки.
- «Переместить только текст» двигает один текст. «Восстановить положение текста» возвращает его к полочке.

**Печать: толщина линий**
- Найдена причина слишком тонких линий. У листов, настроенных в миллиметрах внутри дюймового чертежа (как в «אריה צוק»), PDF получался в 25,4 раза больше листа: A3 был 10,7 × 7,5 м. При печати такой страницы на A3 всё уменьшалось в 25,4 раза, вместе с толщиной линий. Теперь страница PDF имеет размер листа, и толщины линий печатаются как задано.
- Программа читает таблицы стилей печати AutoCAD (.ctb и .stb): цвета, толщины линий, оттенки серого, интенсивность. Если лист ссылается на таблицу, которой нет в программе (у «אריה צוק» это SIVAN.ctb), CADCraft ищет файл рядом с чертежом и в папках стилей печати AutoCAD этого пользователя. Если файл не найден, в командной строке появляется сообщение, и лист печатается с цветами и толщинами самих объектов. Файл .ctb также можно загрузить вручную в «Диспетчере стилей печати».

**Значки**
- «Показать целиком» в предварительном просмотре: четыре прямых угла вокруг прямоугольника.
- «Параметры»: шестерня с зубцами.
- «Отменить» и «Повторить»: толще дуга и крупнее стрелка.
- На панель инструментов можно добавить кнопки «Текстовый стиль», «Размерный стиль» и «Параметры» («Настроить панель инструментов», раздел «Стандартный»).

**Диспетчер блоков**
- Кнопка «Вставить блок» (и команда ВСТАВИТЬ / INSERT) открывает диспетчер блоков панелью, закреплённой справа, рядом со «Слоями» и «Свойствами».
- Значок в правом верхнем углу превращает панель в отдельное окно, а в окне — обратно в панель. Программа запоминает, как диспетчер открывался в последний раз.
- Команда -ВСТАВИТЬ (-INSERT) по-прежнему спрашивает имя блока в командной строке.

### English

**Text and leader properties**
- The Properties palette has a Style row (text style) for text, multiline text and multileaders. It works for several selected objects at once and shows *VARIES* when their styles differ.
- The arrowhead lists (multileaders in Properties, the dimension style editor) show a picture of each arrowhead, user arrowhead blocks included.

**The multileader text grip**
- Multileader text has its own grip, with Move with Leader, Move Text Only and Reset Text Position.
- Move with Leader puts the text at the picked point; the landing goes beside the text and the leader runs to it from its last point.
- Move Text Only moves the text alone; Reset Text Position puts it back beside the landing.

**Printing: lineweights**
- Found why lines printed too thin: a layout set up in millimetres in an inch drawing (as in "אריה צוק") produced a PDF page 25.4 times the sheet (A3 came out 10.7 × 7.5 m), so printing it on A3 shrank everything, lineweights included, 25.4 times. The PDF page is now the size of the sheet and lineweights print as set.
- AutoCAD plot style tables (.ctb and .stb) are read: colours, lineweights, grayscale and screening. When a layout names a table CADCraft doesn't have (SIVAN.ctb for "אריה צוק"), it looks next to the drawing and in this user's AutoCAD plot style folders. If it isn't found, the command line says so and the layout plots with the objects' own colours and lineweights. A .ctb file can also be loaded in the Plot Style Manager.

**Icons**
- Zoom to Fit in Plot Preview: four right-angle corners around a rectangle.
- Options: a gear with teeth.
- Undo and Redo: a thicker arc and a larger arrowhead.
- Text Style, Dimension Style and Options buttons can be added to the Tool Bar (Customize Tool Bar, Standard section).

**Block Manager**
- The Insert Block button (and INSERT typed) opens the Block Manager as a panel pinned on the right, beside Layers and Properties.
- The icon at its top right turns the panel into a window, and the window back into a panel. The last choice is remembered.
- -INSERT still asks for the block name on the command line.

---

## 21.10.2026 (0.9.0-b11)

### Русский

**Меню приложения (кнопка CADCraft)**
- Строки стали выше, текст крупнее. Слева от каждого пункта крупный значок.

**Значки**
- «Сохранить как»: крупный карандаш рядом с дискетой.
- «Импорт» и «Экспорт»: крупные стрелки.

**Текст (TEXT) и мультивыноска (MLEADER)**
- Пока вы указываете точку вставки, высоту и угол текста, рядом с курсором видна рамка, которую займёт строка.
- После этого открывается окно редактирования текста прямо на чертеже, как у многострочного текста. Enter завершает строку, и окно открывается для следующей строки под ней. Enter в пустой строке завершает команду.
- У мультивыноски, пока вы указываете точки, виден отрезок до курсора. После последней точки открывается окно редактирования текста.
- В палитре «Свойства» у мультивыноски появился тип выноски «Нет». Линия выноски и полка скрываются, но не удаляются: если снова выбрать «Прямая» или «Сплайн», выноска вернётся. В файл DXF записывается тип выноски «невидимая».

**Диспетчер блоков**
- Кнопка «Блоки» открывает диспетчер блоков: слева список блоков чертежа с поиском, справа предпросмотр выбранного блока, число объектов и вхождений, базовая точка, описание.
- Кнопки: «Вставить» (или двойной щелчок по блоку), «Редактировать блок», «Переименовать», «Удалить» (только неиспользуемый блок), «Сохранить в файл» (WBLOCK), «Создать».

**Предварительный просмотр печати**
- Сверху кнопки «Увеличить» и «Уменьшить» (лупа с плюсом и с минусом) и «Показать целиком». Колёсико мыши увеличивает и уменьшает изображение вокруг курсора.
- Лист можно перетаскивать мышью. Когда изображение больше окна, снизу и справа появляются полосы прокрутки.
- Окно можно растягивать. Изображение рисуется в более высоком разрешении.

**Пакетная печать — менеджер печати**
- Пакетная печать открывается в отдельном окне. В нём список листов текущего чертежа в порядке вкладок.
- Порядок меняется перетаскиванием строки или кнопками «Переместить в самый верх», «Переместить выше», «Переместить ниже», «Переместить в самый низ».
- Правый щелчок по листу: «Дублировать» (лист напечатается ещё раз) и «Удалить».
- В разделе «Добавить листы из» можно выбрать любой открытый чертёж и добавить его листы по одному или все сразу. По умолчанию листы других чертежей в список не попадают.
- Внизу выбор: «Один файл PDF со всеми листами» или «Отдельный файл PDF для каждого листа».

**Панель «Слои»**
- Если слоёв много, у списка текущего слоя появляется полоса прокрутки.
- В панель можно добавить выпадающие списки «Текущий текстовый стиль», «Текущий размерный стиль» и «Текущий тип линий» (в «Настроить панель инструментов», раздел «Слои»). В новой настройке они уже стоят в конце панели. У длинных списков тоже есть прокрутка.

### English

**Application menu (the CADCraft button)**
- Taller rows, larger text, and a large icon to the left of each item.

**Icons**
- Save As: a large pencil beside the floppy disk.
- Import and Export: large arrows.

**TEXT and MLEADER**
- While you pick the text's start point, height and angle, a frame by the cursor shows where the line will go.
- Then the text editor opens right on the drawing, as for multiline text. Enter finishes the line and the editor opens for the next line below it. Enter on an empty line ends the command.
- While you pick a multileader's points, the segment to the cursor is shown. After the last point the text editor opens.
- The Properties palette offers leader type None for multileaders. The leader line and landing are hidden, not deleted: choosing Straight or Spline again brings them back. DXF files store it as an invisible leader.

**Block Manager**
- The Blocks button opens a Block Manager: the drawing's blocks in a list with search on the left, and on the right a preview of the selected block, its object and reference counts, base point and description.
- Buttons: Insert (or double-click a block), Edit Block, Rename, Delete (only a block nothing uses), Save to File (WBLOCK), Create.

**Plot Preview**
- Zoom In and Zoom Out buttons (a magnifier with plus and minus) and Zoom to Fit at the top. The mouse wheel zooms about the cursor.
- Drag the sheet to move it. Scroll bars appear at the bottom and on the right when the image is larger than the window.
- The window can be resized, and the image is rendered at a higher resolution.

**Batch Plot is now a print manager**
- Batch Plot opens its own window, listing the current drawing's sheets in tab order.
- Change the order by dragging a row or with Move to Top, Move Higher, Move Lower and Move to Bottom.
- Right-click a sheet for Duplicate (it prints again) and Delete.
- Under "Add sheets from" choose any open drawing and add its sheets one by one or all at once. Other drawings' sheets are not in the list unless you add them.
- At the bottom choose "One PDF file with all sheets" or "A separate PDF file for each sheet".

**Layers bar**
- The current-layer list scrolls when there are many layers.
- Current Text Style, Current Dimension Style and Current Linetype drop-downs can be added to the bar (Customize Tool Bar, Layers section). A fresh setup has them at the end of the bar. Long lists scroll too.

---

## 20.10.2026 (0.9.0-b10)

### Русский

**Шрифты SHX**
- Текст в стилях со шрифтами SHX (romans.shx, simplex.shx, txt.shx, ивритские шрифты вроде GIL.SHX и другие) теперь рисуется этими шрифтами, если файл .shx лежит в одной из папок шрифтов («Инструменты ▸ Параметры ▸ Шрифты»). Читаются обычные шрифты SHX, Unicode-шрифты и big fonts (BIGFONT).
- Если в шрифте нет нужного символа, он берётся из встроенного шрифта CADCraft.
- Строка с ивритом выкладывается справа налево, как в AutoCAD.
- Найденные шрифты SHX больше не попадают в сообщение «Не найдены шрифты».

**Штриховка: вкладка «Свои образцы»**
- В окне Hatch and Gradient («Штриховка и градиент») появилась третья вкладка «Свои образцы» (Custom Patterns). Укажите папку с файлами .pat (вручную или кнопкой «Обзор…»), и программа покажет все образцы из этих файлов списком.
- Рядом со списком видно, как выглядит выбранный образец.
- На вкладке есть угол, масштаб и исходная точка, как на вкладке «Шаблон». Справа те же разделы Boundaries и «Параметры».
- Папка запоминается. Кнопка «Обновить» перечитывает её после того, как вы добавили или изменили файлы.

**Листы (layouts)**
- Найдена причина, по которой листы из «אריה צוק» выглядели меньше, чем в AutoCAD. Чертёж в дюймовой системе (MEASUREMENT = 0), а листы настроены в миллиметрах. CADCraft брал единицы листа из настроек чертежа, и лист A3 получался 16,5 × 11,7 единицы вместо 420 × 297. Теперь единицы бумаги берутся из настроек самого листа, как в AutoCAD, и рамка помещается на лист так же.
- При сохранении эти единицы записываются в файл.

**Темы оформления**
- Добавлены темы «Чёрная» и «Светло-голубая» (#85abf6). Их можно выбрать в «Инструменты ▸ Параметры» или в меню «Вид ▸ Тема интерфейса».

**Ввод у курсора**
- Текст, который вы набираете в командной строке, теперь дублируется в окошке рядом с курсором: число сторон многоугольника, имя, параметр или имя команды. Окошко показывается и при выключенном динамическом вводе (F12).

### English

**SHX fonts**
- Text in styles that use SHX fonts (romans.shx, simplex.shx, txt.shx, Hebrew fonts such as GIL.SHX and others) is now drawn with those fonts when the .shx file is in one of the font folders (Tools ▸ Options ▸ Fonts). Regular SHX shape fonts, Unicode fonts and big fonts are read.
- Characters a font lacks are taken from CADCraft's built-in font.
- A line with Hebrew is laid out right to left, as in AutoCAD.
- SHX fonts that are found no longer appear in the "Fonts not found" list.

**Hatch: Custom Patterns tab**
- The Hatch and Gradient window has a third tab, Custom Patterns. Point it at a folder of .pat files (type the path or use Browse…) and it lists every pattern in those files.
- A preview of the selected pattern is shown beside the list.
- The tab has angle, scale and hatch origin, as the Pattern tab does. Boundaries and Options are on the right as before.
- The folder is remembered. Reload reads it again after you add or change files.

**Layouts**
- Found why the layouts of "אריה צוק" looked smaller than in AutoCAD. The drawing is in inches (MEASUREMENT = 0) but its layouts are set up in millimetres. CADCraft took the paper units from the drawing, so an A3 sheet came out 16.5 × 11.7 units instead of 420 × 297. Paper units now come from the layout's own settings, as in AutoCAD, and the title block fits the sheet the same way.
- These units are written back when the drawing is saved.

**Interface themes**
- Two new themes: Black and Light Blue (#85abf6). Choose them in Tools ▸ Options or View ▸ Interface Theme.

**Typed input by the cursor**
- What you type at the command line is now repeated in a small box next to the cursor: a polygon's number of sides, a name, an option or a command name. The box shows even with Dynamic Input (F12) off.

---

## 20.10.2026 (0.9.0-b9)

### Русский

**Палитра «Наборы инструментов» (Tool Sets)**
- Палитру можно превратить в ленту (ribbon), как в Microsoft Office. Лента встаёт прямо под строкой меню «Файл, Правка, Вид…».
- У ленты вкладки по задачам: «Рисование», «Модификация», «Аннотации», «Блок», «Параметризация», «3D-моделирование». На вкладке инструменты сгруппированы, название группы подписано снизу. Крупные кнопки подписаны, мелкие стоят столбиками по три.
- Если перетащить палитру к верхнему краю чертежа, она тоже становится лентой.
- Правый щелчок в любом месте палитры или ленты открывает меню: «Плавающая панель», «Закрепить слева», «Превратить в ленту», «Переместить в группу» (к слоям и свойствам или к другой группе панелей), «Закрыть панель». Пункта, который уже действует, в меню нет.
- Правый щелчок по кнопке инструмента показывает это же меню, а под ним прежние пункты раздела «Показывать на панели».

**Обрезать и Удлинить (TRIM / EXTEND)**
- Если перед запуском команды объекты уже выделены, они становятся режущими (граничными) кромками. Пока команда работает, они остаются подсвеченными. Подсветка снимается, когда команда завершается или отменяется.
- Щелчок в пустом месте начинает линию-«забор». Второй щелчок задаёт её конец, и всё, что эта линия пересекает, обрезается (удлиняется). Раньше в этом случае открывалась прямоугольная рамка. Рамка по-прежнему доступна через параметр Crossing в командной строке.

**Значки**
- «Переместить»: крупные стрелки.
- «Повернуть»: стрелка на конце дуги.
- «Обрезать»: ножницы, разрезающие линию надвое.
- «Удлинить»: крупная стрелка.
- «Параллельный» размер (Aligned): значок повёрнут на 13°.

### English

**The Tool Sets palette**
- The palette can turn into a ribbon, as in Microsoft Office. The ribbon sits right under the File, Edit, View… menu bar.
- The ribbon has a tab per task: Draw, Modify, Annotate, Block, Parametric, 3D Modeling. On each tab the tools are grouped, with the group's name underneath. Large buttons have labels, small ones are stacked in columns of three.
- Dragging the palette to the top edge of the drawing also turns it into the ribbon.
- Right-clicking anywhere in the palette or the ribbon opens a menu: Float Panel, Pin to Left, Transform to Ribbon, Move to Group (beside Layers/Properties or another panel group), Close Panel. The option already in effect is left out.
- Right-clicking a tool button shows the same menu, with the existing "Show on Tool Bar" items under it.

**TRIM / EXTEND**
- Objects selected before you start the command become its cutting (boundary) edges. They stay highlighted while the command runs, and the highlight goes away when the command ends or is cancelled.
- A click in empty space starts a fence line. A second click sets its end point, and everything the line crosses is trimmed (extended). Before this, a click in empty space opened a rectangular window. The window is still available through the Crossing option.

**Icons**
- Move: larger arrows.
- Rotate: an arrowhead at the end of the arc.
- Trim: scissors cutting a line in two.
- Extend: a larger arrow.
- Aligned dimension: the icon is turned 13°.

---

## 19.10.2026 (0.9.0-b8)

### Русский

**Размеры: ручка текста**
- «Растянуть» двигает размерную линию вместе с текстом поперёк. Текст при этом остаётся на своём месте относительно стрелок.
- «Переместить с размерной линией» ставит текст туда, где курсор, в том числе за стрелки. Размерная линия продлевается до текста.
- Пока вы тянете ручку, видно то же самое, что получится после щелчка.

**Мультивыноски в палитре «Свойства»**
- Можно менять стиль мультивыноски, тип выноски (прямая или сплайн), стрелку, размер стрелки, длину полки и высоту текста.
- Стрелку можно выбрать из того же списка, что у размеров, включая пользовательские блоки-стрелки чертежа.
- Стрелки мультивыносок из файлов DWG и DXF теперь читаются и сохраняются. Раньше все мультивыноски рисовались с замкнутой залитой стрелкой.

**Копирование между чертежами**
- Ctrl+C в одном открытом чертеже и Ctrl+V в другом теперь работают. Раньше после щелчка по вкладке другого чертежа программа не реагировала на Ctrl+V и на ввод команд.

**Блоки на листах**
- Двойной щелчок по блоку на листе (layout) открывает редактор блоков, как в пространстве модели.

**Имена файлов на иврите**
- На вкладках, в списке недавних файлов и в окне «Сохранить изменения?» имя читается так же, как в проводнике: буквы справа налево, «.dwg» справа.

### English

**Dimensions: the text grip**
- Stretch moves the dimension line across together with the text. The text keeps its place relative to the arrowheads.
- Move with Dim Line puts the text where the cursor is, even past the arrowheads. The dimension line is extended to reach the text.
- While you drag the grip, you see exactly what you will get when you click.

**Multileaders in the Properties palette**
- You can change the multileader style, the leader type (straight or spline), the arrowhead, the arrowhead size, the landing distance and the text height.
- The arrowhead list is the same one dimensions use, including the drawing's own arrowhead blocks.
- Multileader arrowheads are now read from and saved to DWG and DXF files. Before this, every multileader was drawn with a closed filled arrowhead.

**Copying between drawings**
- Ctrl+C in one open drawing and Ctrl+V in another now works. Before this, after clicking another drawing's tab, the program ignored Ctrl+V and typed commands.

**Blocks on layouts**
- Double-clicking a block on a layout opens the Block Editor, as it does in model space.

**Hebrew file names**
- On the drawing tabs, in the recent files list and in the "Save changes?" window, the name reads as it does in File Explorer: letters right to left, ".dwg" on the right.

---

## 19.10.2026 (0.9.0-b7)

### Русский

**Размеры**
- Пользовательские стрелки (блоки чертежа, например hatz или DIM1) рисуются правильно, с нужным размером и поворотом. Раньше они заменялись на «Замкнутую залитую».
- В «Свойствах» и в редакторе размерного стиля такая стрелка показывается как «Пользовательская: имя блока», и её можно выбрать из списка.
- «Переместить с выноской»: под текстом рисуется полочка, а от размерной линии к ней идёт выноска.
- Ручка текста стоит там, где текст виден на экране.
- Текст, который в AutoCAD сдвинут вдоль размерной линии, после открытия файла остаётся на месте и не прыгает в середину при первой правке.

**Блоки**
- Двойной щелчок по блоку открывает редактор блоков. Если у блока есть атрибуты, открывается их редактирование. Внешние ссылки двойной щелчок не трогает.

### English

**Dimensions**
- User arrowheads (blocks of the drawing, such as hatz or DIM1) are drawn correctly, at the right size and angle. Before this, they were replaced with "Closed filled".
- In Properties and in the dimension style editor such an arrowhead shows as "User Arrow: block name" and can be picked from the list.
- Move with Leader: a landing line is drawn under the text, with a leader from the dimension line to it.
- The text grip sits where the text appears on screen.
- Text that was moved along the dimension line in AutoCAD stays in place after the file is opened, and no longer jumps to the middle on the first edit.

**Blocks**
- Double-clicking a block opens the Block Editor. If the block has attributes, their editor opens instead. External references are left alone.

---

## 19.10.2026

### Русский

**Размерные стили**
- Новый диспетчер размерных стилей: создание, изменение, сравнение стилей и выбор текущего.
- Редактор стиля с вкладками: «Линии», «Символы и стрелки», «Текст», «Размещение», «Основные единицы», «Альтернативные единицы», «Допуски».
- В «Свойствах» у выбранного размера видны настройки его стиля (стрелки, линии, текст и другие). Изменение настройки применяется только к этому размеру, как переопределение в AutoCAD.
- У ручки текста размера появилось меню: «Переместить с размерной линией», «Переместить только текст», «Переместить с выноской», «Над размерной линией», «По центру по вертикали» и «Восстановить положение текста».

**Интерфейс**
- Кнопка приложения (меню «Файл») стоит первой на панели инструментов. В меню есть пакетная печать.
- Окно «Параметры» больше не меняет размер при переходе между вкладками.

### English

**Dimension styles**
- New Dimension Style Manager: create, modify and compare styles, and set the current one.
- Style editor with tabs: Lines, Symbols and Arrows, Text, Fit, Primary Units, Alternate Units, Tolerances.
- In Properties, a selected dimension shows its style settings (arrowheads, lines, text and more). Changing a setting changes only that dimension, like an override in AutoCAD.
- The dimension text grip has a menu: Move with Dim Line, Move Text Only, Move with Leader, Above Dim Line, Center Vertically and Reset Text Position.

**Interface**
- The application button (the File menu) is the first button on the Tool Bar. Its menu includes Batch Print.
- The Options window keeps the same size on every tab.

---

## 18.10.2026

### Русский

**Панели инструментов**
- Панель инструментов можно закрепить сверху или слева, перетащив её за маркер.
- Кнопки можно переставлять перетаскиванием, добавлять и убирать в окне «Настроить панель инструментов». Там же можно вернуть всё по умолчанию.
- Подсказки у кнопок показывают название команды, её псевдоним и сочетание клавиш.
- Отдельные панели «Наборы инструментов» и «Слои» тоже закрепляются сверху или слева и настраиваются. На панели «Слои» есть выпадающий список текущего слоя.

**Шрифты**
- В «Инструменты ▸ Параметры ▸ Шрифты» можно добавить свои папки со шрифтами (.ttf, .otf, .ttc). Они просматриваются раньше системных шрифтов.
- После открытия чертежа программа сообщает, каких шрифтов не хватает и каким шрифтом рисуется такой текст.

**Редактирование текста на месте**
- Двойной щелчок по тексту, многострочному тексту, размеру, мультивыноске или ячейке таблицы открывает редактор прямо на чертеже.
- В ячейках таблицы Tab переходит к следующей ячейке, Shift+Tab к предыдущей, Alt+↑/↓ к ячейке выше или ниже.
- В тексте размера можно вставить измеренное значение (<>) или вернуть только измеренное значение.

**Редактор текста**
- Панель редактора нарисована заново, кнопки стоят в два ряда. Есть кнопка закрытия.
- Добавлено меню «Дополнительные символы»: ≈, ≠, ≤, ≥, ×, ÷, √, ∞, Δ, Ω, µ, π, α, β, γ, φ, ², ³, ½, ¼, ¾, ‰ и другие.
- Если в шрифте текста нет нужного символа, он берётся из запасного шрифта, а не пропадает.

### English

**Tool bars**
- The Tool Bar can be docked at the top or on the left by dragging its handle.
- Buttons can be reordered by dragging, and added or removed in the Customize Tool Bar window. You can also reset everything to the defaults there.
- Button tooltips show the command name, its alias and its keyboard shortcut.
- The separate Tool Sets and Layers tool bars can also be docked at the top or on the left, and customized. The Layers bar has a current layer drop-down.

**Fonts**
- In Tools ▸ Options ▸ Fonts you can add your own font folders (.ttf, .otf, .ttc). They are searched before the system fonts.
- After a drawing opens, the program reports which fonts are missing and which font that text is drawn with instead.

**Editing text in place**
- Double-clicking text, multiline text, a dimension, a multileader or a table cell opens the editor right on the drawing.
- In table cells, Tab moves to the next cell, Shift+Tab to the previous one, Alt+↑/↓ to the cell above or below.
- In dimension text you can insert the measured value (<>) or go back to the measured value only.

**Text editor**
- The editor's toolbar has been redrawn, with buttons in two rows. There is a close button.
- New Additional symbols menu: ≈, ≠, ≤, ≥, ×, ÷, √, ∞, Δ, Ω, µ, π, α, β, γ, φ, ², ³, ½, ¼, ¾, ‰ and more.
- When the text's font has no glyph for a symbol, it is taken from a fallback font instead of disappearing.
