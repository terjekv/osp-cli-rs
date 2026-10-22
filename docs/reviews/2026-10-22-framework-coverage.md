# Framework coverage boundary review

Measured 2026-10-22 at `ddb9cddac38084989914cb04f2d249def985e4db`.
Product caller audit: `osp-cli` branch `refactor/service-crates`,
`50fb16065dd95379045941f76f395bf538ade65f`; its unrelated workflow edit
was left untouched.

## Baseline

Pinned `nightly-2026-10-16`, cargo-llvm-cov 0.9.1, full `coverage` check:
32,065 / 38,863 lines (82.5078%). The unchanged 85% gate required 969 more
covered lines. All baseline tests passed; only the coverage floor failed.

Counts below come from LLVM native file summaries. Missing source ranges use
cargo-llvm-cov's zero-execution function-region algorithm, filtered to files in
`data[0].files`. Region ranges include braces and need not equal executable
line counts. All ranges refer to the baseline revision, not this branch's
subsequent test deletions. No production exclusion or policy was changed.

## Modules ranked by uncovered executable lines

| Module | Uncovered | Executable |
| --- | ---: | ---: |
| `src/repl` | 1,423 | 8,959 |
| `src/ui` | 1,219 | 6,607 |
| `src/app` | 1,080 | 6,158 |
| `src/cli` | 899 | 3,072 |
| `src/config` | 519 | 4,192 |
| `src/context.rs` | 339 | 526 |
| `src/dsl` | 334 | 3,734 |
| `src/core` | 209 | 1,764 |
| `src/diagnostics.rs` | 206 | 283 |
| `src/completion` | 187 | 2,023 |
| `src/native.rs` | 180 | 626 |
| `src/plugin` | 65 | 556 |
| `src/snapshot.rs` | 62 | 62 |
| `src/command_words.rs` | 28 | 116 |
| `src/startup_trace.rs` | 28 | 144 |
| `src/lib.rs` | 20 | 20 |
| `src/bin` | 0 | 2 |
| `src/normalize.rs` | 0 | 19 |

## Files ranked by uncovered executable lines

| File | Uncovered | Executable | Covered | Missing source ranges |
| --- | ---: | ---: | ---: | --- |
| `src/context.rs` | 339 | 526 | 35.55% | 32–36, 38–40, 42, 64, 66–68, 70–72, 87–88, 90–93, 102, 117–124, 126–128, 142–149, 152–157, 159–163, 165–168, 170–178, 183, 188, 191–195, 251, 271, 278–298, 300–301, 303, 305, 307–313, 315–322, 325–334, 336–338, 340–349, 351–357, 359–367, 382–384, 386–389, 391, 393, 395–407, 409–410, 412–413, 421, 427–428, 432–435, 439–442, 447, 451–457, 459–465, 467–470, 473–474, 476–477, 479–480, 484–489, 491–497, 511, 513–519, 525–529, 531, 536–547, 549–550, 556–561, 564–576, 578–582, 584–589, 601–615, 617–620, 622–624, 626–627 |
| `src/app/external.rs` | 329 | 1161 | 71.66% | 72, 77–83, 88–95, 101, 110, 115–119, 178–179, 185–188, 190–192, 196, 206, 208, 217, 231, 251–253, 287–294, 361–362, 364, 369, 375–381, 403–405, 441, 457, 474–479, 485, 540–544, 577, 580–590, 592–595, 597, 599–624, 626–627, 651, 680–685, 688, 702, 704–705, 707, 709–717, 755, 770, 773–775, 777–785, 787–790, 792, 809, 841, 847–849, 851, 897, 900, 910–914, 940, 971–974, 994–995, 1020–1021, 1023, 1025, 1040, 1052, 1058–1059, 1085, 1087, 1106–1109, 1111–1113, 1124, 1141, 1149–1151, 1185–1190, 1197–1202, 1208, 1210–1218, 1235–1248, 1250–1262, 1281, 1307, 1311, 1316–1324, 1326, 1338, 1347, 1350–1352, 1354–1355, 1377 |
| `src/cli/commands/config.rs` | 251 | 1068 | 76.50% | 54, 183–185, 213, 229–232, 279, 284, 322–325, 327, 352, 417–422, 450, 487–500, 502–506, 508–532, 534–535, 537–539, 549, 569, 573–600, 613–623, 626, 637–653, 656, 658–664, 666–679, 681, 683, 689, 696, 701–703, 705, 707, 712–717, 729, 756, 758, 763, 780, 792, 799–801, 805, 831–835, 860, 869, 889–892, 894, 911, 923, 927, 981, 987, 996, 998–1000, 1009, 1011–1012, 1014–1016, 1018, 1153, 1157, 1163, 1222, 1316, 1336–1357, 1359–1360 |
| `src/ui/projection.rs` | 241 | 824 | 70.75% | 48–50, 86–89, 100, 116–119, 200–206, 208–210, 212–214, 216, 220, 223, 259–264, 266–272, 274, 276, 278–283, 384–392, 398–402, 594, 599, 601–604, 606–608, 610, 612–613, 615–618, 620–625, 627, 650–652, 654–655, 662–665, 667–669, 671–672, 674–676, 678–681, 684–685, 687–688, 690–693, 695–696, 699–700, 702–704, 707–708, 710–712, 715–716, 719–722, 724, 726, 728–731, 734–738, 740–748, 757, 764, 767–768, 770–771, 773–776, 778, 780, 782–785, 801, 845, 847–859, 861–863, 878, 885–886, 896, 907, 928–930, 932–934, 936–939, 942–948, 950–955, 959–960, 963, 965–966, 968–972, 974–976, 978–979, 982–984, 1047 |
| `src/ui/picker.rs` | 222 | 222 | 0.00% | 41–43, 45–47, 49–56, 58, 60, 66–68, 70–72, 74–76, 78–80, 89–105, 107–129, 131–132, 134–137, 139, 142–149, 151–155, 157, 159–161, 163–169, 171–182, 184, 187–188, 190–193, 195, 198, 200–202, 204, 206–207, 209–216, 218–226, 228–229, 232–233, 235–236, 239–241, 243–245, 247, 250–251, 254, 257–259, 261, 263, 265–277, 279–283, 288–294, 296–301, 304–305, 307, 309, 312–313, 315, 318–319, 321, 323–325, 327, 334–340, 344–349 |
| `src/diagnostics.rs` | 206 | 283 | 27.21% | 158–167, 169, 173–176, 178–183, 185, 188, 230, 232, 242–245, 247–250, 258, 260–263, 265, 267–269, 271, 273–274, 283–288, 290–298, 310–315, 318, 320, 322–323, 325–337, 339–355, 363–367, 370, 372, 374–384, 396–399, 402, 434–448, 450–457, 459–467, 469–470, 472, 483–490, 492, 494, 497, 501–513, 531–533, 535, 537–540, 592–595, 604–610 |
| `src/repl/recording.rs` | 201 | 253 | 20.55% | 53–64, 68, 70–76, 82, 86–88, 91–98, 100, 115–121, 123–131, 133–134, 136–138, 140–144, 152–168, 175–177, 179–180, 184–185, 188–189, 191–192, 194, 196–200, 202–206, 208–214, 216–221, 224–229, 231–240, 242, 244, 247–248, 250–254, 257–258, 260–261, 265–266, 270–271, 274, 276, 278–279, 281–283, 285, 289–290, 292, 302, 308, 312–317, 321–345, 372–377, 379 |
| `src/cli/commands/config/picker.rs` | 193 | 193 | 0.00% | 14–27, 30–35, 37, 39–43, 45, 47–54, 56–61, 63–68, 70, 72, 75–84, 86–90, 92–98, 100–107, 110–117, 119–131, 134–141, 161–179, 183–185, 187–195, 197, 199–209, 212–213, 215–217, 219, 221, 223–225, 227–233, 235–247, 249, 251–257 |
| `src/app/sink.rs` | 184 | 366 | 49.73% | 67–69, 85–87, 94–96, 99, 104–106, 133–134, 174–179, 187–188, 193, 207–210, 212, 214–221, 223–227, 229–233, 235–242, 244–247, 249, 251–262, 266–277, 302, 306, 322, 340, 342–346, 350–358, 360–364, 369–372, 397, 409–416, 421, 488–491, 497, 499, 510, 522–526, 528–533, 535–572, 574–576, 578 |
| `src/native.rs` | 180 | 626 | 71.25% | 128–130, 139–141, 144–146, 150–153, 156–158, 163–167, 205, 213–215, 226, 233–236, 238, 242–248, 250, 263, 270–274, 276, 284, 352–354, 360–362, 365–368, 371–374, 377–379, 382–384, 504, 524–525, 528, 558, 562–569, 602–603, 610, 614–622, 633–637, 658, 672–677, 684, 700, 731, 775–777, 779, 781, 907–909, 922–924, 934–936, 965–967, 970–972, 979–985, 992, 1011–1013, 1168–1170, 1172, 1174–1176, 1178–1189, 1191–1196, 1200–1202, 1234, 1239, 1310 |
| `src/config/core.rs` | 165 | 1208 | 86.34% | 161–164, 231, 469–473, 522–525, 600–601, 784, 981–984, 987–993, 1025, 1096, 1154, 1176–1178, 1197–1199, 1250, 1274, 1519–1520, 1522, 1651, 1658–1660, 1799–1805, 1812–1816, 1846–1849, 1853–1857, 1894–1898, 1916, 1918–1919, 1922–1926, 1961–1964, 1967–1970, 1993, 1995–2002, 2020–2041, 2046–2052, 2054–2056, 2058–2060, 2062–2065, 2070, 2094–2098, 2100–2103, 2151, 2153 |
| `src/app/host.rs` | 157 | 1070 | 85.33% | 103, 175–177, 179–181, 183–185, 187–189, 191–193, 195–197, 199–201, 203–205, 272, 288–295, 298–302, 304, 306, 321, 331–337, 349–351, 375–382, 385–388, 390–391, 427, 470, 498, 510, 524, 566–567, 575, 586–594, 596–602, 604, 657–659, 700–701, 704–708, 719, 723, 896, 919, 969, 976, 988, 1005, 1009, 1011, 1015, 1035–1036, 1060, 1064, 1114–1115, 1117, 1119, 1121–1123, 1139–1141, 1143–1144, 1146, 1158, 1160–1162, 1164–1165, 1168, 1170, 1179, 1191, 1197, 1202, 1204, 1214, 1254, 1367, 1369, 1407, 1409–1412 |
| `src/cli/commands/theme/preview.rs` | 157 | 157 | 0.00% | 16–30, 32–34, 36–54, 56–73, 75–86, 88–107, 119–132, 134–143, 146–149, 151, 153, 155–159, 161, 163–164, 166–181, 183–194, 196 |
| `src/repl/engine/session.rs` | 150 | 447 | 66.44% | 52, 59–60, 92, 98, 106, 113, 213–217, 250, 253–258, 368–382, 384–385, 387–401, 403, 406–407, 409–411, 413–414, 416–418, 432–434, 436, 438–439, 441–446, 448–449, 451–452, 455–462, 472–473, 493–497, 504, 506–507, 509–510, 512, 514–520, 532, 554, 563, 567–570, 572–577, 580, 582–602, 604–605, 607 |
| `src/repl/dispatch/builtins.rs` | 138 | 388 | 64.43% | 50–54, 59–66, 68–76, 96, 124–126, 128–129, 134, 136, 142–146, 148, 176–181, 183–186, 188–190, 192–193, 195–219, 221–243, 245–247, 250–255, 259–264, 282–285, 295, 381, 383–385, 451, 496–497, 499, 502–504 |
| `src/repl/dispatch.rs` | 100 | 517 | 80.66% | 180, 203, 205, 207, 209, 213–217, 234, 247, 256, 260, 274, 276–292, 296, 323–324, 326, 341, 347–359, 362–363, 379–382, 388–389, 391, 401, 418, 457–460, 467–468, 470, 475–478, 521, 525, 527–529, 531, 536–539, 555–558, 680 |
| `src/ui/prompt.rs` | 92 | 136 | 32.35% | 12–13, 15–18, 22–24, 37–41, 43–48, 50–56, 70–71, 87, 96, 102–108, 112–115, 117–124, 126–131, 135–140, 142, 146–151, 153–154, 158, 160, 162, 166–174, 179–181, 196–200 |
| `src/core/plugin.rs` | 91 | 444 | 79.50% | 181–184, 188, 190, 192–193, 195, 198, 253, 255–256, 299, 303, 312–316, 571, 588–593, 595, 597, 605, 638–641, 659–660, 662, 665–668, 670, 672, 679, 693, 695, 749, 807, 809, 885–887, 889–894, 896–900, 967–971, 974–975, 978–980, 983, 995, 998, 1006–1008, 1022, 1031, 1034, 1041, 1054, 1072, 1074–1076 |
| `src/repl/presentation.rs` | 89 | 629 | 85.85% | 48, 73–74, 76, 78, 91, 99, 202, 230, 258, 260–270, 303–310, 312, 334, 342, 385, 388, 393–396, 467, 503, 633, 637–638, 643–651, 655, 657, 662–669, 671–675, 677, 679–684, 686–688, 875 |
| `src/app/command_output.rs` | 88 | 777 | 88.67% | 34, 47–48, 137–139, 159, 164, 167, 198, 220, 294, 298–299, 302, 310, 347, 359, 421, 445, 447, 451, 497, 499, 519, 531, 545, 579, 642, 660, 666, 695, 704, 706–708, 710–712, 715, 717, 746–747, 797, 799, 815–822, 826–831, 836–840, 868–873, 888, 896–897, 902, 908, 925, 928, 936, 947–948 |
| `src/config/store.rs` | 83 | 623 | 86.68% | 39–42, 69–71, 226–228, 241, 253, 256–257, 260, 263–264, 323, 333, 336–340, 367–370, 376–379, 384, 386, 393, 401–403, 436, 441–443, 451, 480, 486, 493, 528, 537–540, 683, 706–707, 709, 718–721, 802–803, 805, 841–844, 854, 891, 895, 906, 934 |
| `src/app/mod.rs` | 81 | 248 | 67.34% | 248–254, 289–297, 299–308, 312–328, 332–337, 377–380, 382–386, 462, 607–609, 694, 704–713, 715, 717, 719–724 |
| `src/ui/lower.rs` | 79 | 670 | 88.21% | 89, 114–123, 131, 138–144, 146–150, 217–219, 309, 369, 417, 490, 501, 503–507, 512–520, 536–539, 547, 549–553, 567, 594, 604, 623, 636–641, 644, 646–650, 762, 859–860 |
| `src/repl/history_store.rs` | 78 | 1003 | 92.22% | 53, 55, 68, 76, 83, 186, 351, 361, 368–371, 378, 391, 401–403, 424, 429–430, 451–452, 476, 482, 556, 577, 649, 658–660, 680–681, 689, 712, 726, 776, 782, 804, 911, 920, 957, 959–960, 964–965, 968, 985–990, 998, 1066, 1092, 1121, 1206, 1225, 1256, 1259, 1282, 1298, 1318–1319, 1325 |
| `src/repl/host.rs` | 78 | 265 | 70.57% | 129, 133–144, 146–148, 151–164, 170–173, 175–178, 181–183, 185–186, 192, 196–197, 200–201, 203, 205, 208–209, 217, 219–224, 229, 231–236, 275, 346–353 |
| `src/repl/notices.rs` | 76 | 161 | 52.80% | 25–27, 29, 34, 42, 90–92, 95–103, 109–114, 117–123, 137, 162, 167–172, 177, 185, 189, 194, 196–203, 205–213, 215–217, 232 |
| `src/ui/mod.rs` | 71 | 160 | 55.62% | 80–84, 87–96, 98, 100, 102–103, 115–126, 128, 130–131, 133, 135, 191, 249–252, 254, 262–263, 265–267, 290–311 |
| `src/config/secrets.rs` | 66 | 385 | 82.86% | 41–44, 89, 95, 200–203, 255, 308, 330, 335, 337–340, 356, 365, 372, 378, 398–402, 448, 476, 497–501, 503–507, 518–521, 535, 543–546, 553, 555–559 |
| `src/cli/commands/doctor.rs` | 65 | 290 | 77.59% | 32–35, 49, 64–68, 70–71, 113–115, 123, 134, 154, 160, 165, 167–170, 179–183, 189, 191, 195–197, 199–204, 206–209, 211–212, 215, 217–219, 239, 306–310, 326, 328 |
| `src/cli/commands/context.rs` | 64 | 64 | 0.00% | 10–16, 18, 21–33, 35, 38–41, 43, 45–48, 50–67, 69, 71, 73–75, 78–86 |
| `src/ui/plan/cards.rs` | 63 | 259 | 75.68% | 93, 113, 131, 138–141, 148, 161, 173–178, 180–186, 188, 191–203, 205–206, 208, 210–212, 214, 216–218, 221–225, 227–230, 232, 234–235, 237, 239–241, 262 |
| `src/completion/model.rs` | 62 | 256 | 75.78% | 233, 238, 243, 246–248, 251–253, 260–262, 266–268, 286–288, 292–294, 383–386, 415–417, 421–423, 431–437, 441–446, 461–468, 470–471, 488, 491 |
| `src/snapshot.rs` | 62 | 62 | 0.00% | 30–37, 39–45, 47–51, 55–58, 61–66, 68–70, 79–92, 94–100, 102–109 |
| `src/completion/suggest.rs` | 57 | 599 | 90.48% | 309, 327, 329, 333, 335, 353, 357, 387, 405–416, 418–421, 439–444, 446, 463–466, 513–514, 516, 550, 679, 716, 720, 813–824, 826 |
| `src/repl/history.rs` | 57 | 278 | 79.50% | 49, 56, 59, 87–96, 134, 202–207, 209, 230–235, 238, 241–249, 252–256, 317–319, 321, 324, 340–347 |
| `src/ui/theme_catalog.rs` | 57 | 398 | 85.68% | 135–141, 148, 203, 257, 324, 382, 518–523, 525–538, 540, 542–544, 549–557, 559–563 |
| `src/dsl/verbs/filter.rs` | 55 | 278 | 80.22% | 86, 141, 163, 194, 208–210, 218–220, 222–232, 234–236, 239–242, 244–245, 247–249, 283–284, 303, 314, 355–357, 365–368, 377, 391, 401, 425 |
| `src/ui/text.rs` | 53 | 273 | 80.59% | 14–17, 19–20, 27–31, 33, 35–41, 43, 147, 185, 192, 217, 219, 224–230, 232, 234–235, 237–238, 264, 274, 285, 303–304, 306, 322, 328, 332–334, 336, 338–341 |
| `src/cli/commands/theme.rs` | 52 | 208 | 75.00% | 26, 28, 64–66, 68–73, 78–89, 118–130, 132–137, 140–141, 143–146, 148, 150 |
| `src/ui/plan/mod.rs` | 52 | 283 | 81.63% | 117–119, 121, 248, 252, 254–256, 258–269, 272–278, 280–285, 288–291, 293, 295–297, 351, 374, 385–386 |
| `src/repl/engine/editor.rs` | 49 | 252 | 80.56% | 68–69, 95–100, 102–104, 163, 166, 189–192, 195–197, 226, 229–232, 243–245, 247–248, 252, 258, 261–262, 264, 266–271, 275, 296–297, 299, 373, 391 |
| `src/repl/lifecycle.rs` | 48 | 273 | 82.42% | 79, 164, 172, 180, 188, 192–195, 210–216, 221, 223–225, 235, 263, 268, 330, 334–343, 345–347, 351–354 |
| `src/app/help.rs` | 45 | 199 | 77.39% | 23–35, 37–41, 43–56, 58, 103–104, 106, 154, 250, 256, 271, 279 |
| `src/app/runtime.rs` | 44 | 337 | 86.94% | 48–49, 468–476, 479–481, 485–487, 490–492, 530–532, 535–537, 561–566, 710–712, 714–720 |
| `src/cli/invocation.rs` | 43 | 346 | 87.57% | 269–270, 272–273, 275, 278–279, 281–282, 284, 287–288, 290–291, 293, 296–297, 299–300, 302, 366, 459, 559–565, 567, 569–570, 572–573 |
| `src/repl/menu_core.rs` | 43 | 659 | 93.47% | 322, 394–401, 428–429, 538, 546, 560, 570, 580, 586, 594, 601, 605, 623, 648, 660, 747, 756–760, 762, 764–765, 810, 914, 916, 918, 921–922, 924, 926–928 |
| `src/repl/menu.rs` | 42 | 594 | 92.93% | 183, 189–190, 272, 294–296, 298–299, 466–468, 470, 472, 475, 477–479, 493, 638–641, 643–645, 647–649, 651–653, 673–676, 701–703, 743, 786, 808 |
| `src/config/loader.rs` | 41 | 331 | 87.61% | 75–77, 106–107, 182–184, 251, 254–256, 298–300, 306–310, 321, 328–330, 400–402, 446–447, 454–455, 598–605 |
| `src/plugin/run.rs` | 41 | 176 | 76.70% | 40, 52–54, 66, 107, 121, 124–127, 132, 147, 159, 165, 169, 226, 235–236, 239–243, 247, 268–271, 274–281 |
| `src/config/resolver.rs` | 40 | 268 | 85.07% | 89–91, 133, 142–144, 162–166, 207, 223, 243, 259–261, 280–283, 358–362, 364, 367–370, 448–454 |
| `src/repl/engine/hint.rs` | 39 | 338 | 88.46% | 101–102, 104–109, 111, 113, 118–121, 123–124, 126, 155, 188, 208, 227, 229, 245, 254, 257–258, 379, 397–403 |
| `src/cli/pipeline.rs` | 38 | 296 | 87.16% | 76–77, 79, 81, 152, 171–172, 184, 186–189, 221–222, 224, 263, 271, 319–321, 326, 329, 331, 386, 391–393, 413–415, 431 |
| `src/config/runtime.rs` | 38 | 287 | 86.76% | 221–225, 268–274, 323, 389–397, 402, 423, 523–525, 527, 583, 603–604 |
| `src/ui/emit/delimited.rs` | 37 | 37 | 0.00% | 4–6, 8, 10–23, 25, 27, 29–43, 47–48 |
| `src/ui/interrupt.rs` | 37 | 46 | 19.57% | 24, 27, 32–37, 45–60, 62–70, 72, 76 |
| `src/ui/section_chrome.rs` | 37 | 294 | 87.41% | 72, 76, 107–110, 112, 289–290, 293, 326, 331, 333, 338, 376–383, 398, 420, 431, 435, 448, 470–479 |
| `src/repl/highlight.rs` | 34 | 282 | 87.94% | 115, 120, 178–179, 267, 272, 276, 293, 307, 379–380, 399–417, 419 |
| `src/dsl/eval/resolve.rs` | 33 | 387 | 91.47% | 59, 71, 90, 123, 129, 131, 146, 154, 180, 304, 317, 349, 377, 413, 424, 455, 479, 486, 488, 501–505, 513, 515, 556, 572, 583, 597, 659, 662 |
| `src/config/bootstrap.rs` | 32 | 166 | 80.72% | 36–39, 43–47, 80–88, 159–161, 177, 181–183, 215–217, 221–224 |
| `src/dsl/eval/matchers.rs` | 32 | 310 | 89.68% | 140, 170, 173–175, 177–181, 212, 217–218, 236–240, 251, 317–318, 326, 341–342, 347, 368, 379, 392, 424, 430, 453, 456 |
| `src/dsl/verbs/sort.rs` | 32 | 192 | 83.33% | 96–99, 120–122, 136–138, 145, 149, 165, 193–199, 218, 242–244, 251–252, 268–269, 277, 284 |
| `src/ui/theme/mod.rs` | 32 | 307 | 89.58% | 76–85, 87, 394, 466–485 |
| `src/cli/mod.rs` | 31 | 220 | 85.91% | 151–153, 213–218, 804–815, 828, 955–959, 965, 967 |
| `src/dsl/verbs/group.rs` | 31 | 223 | 86.10% | 88, 99, 128, 153, 172, 199, 209, 213–215, 222–226, 245, 252–256, 266, 274–276, 282, 295, 312 |
| `src/repl/product.rs` | 31 | 71 | 56.34% | 31, 62–66, 68–73, 80–81, 83, 115–121, 123–129 |
| `src/completion/tree.rs` | 30 | 276 | 89.13% | 50–53, 74–77, 98–100, 102–103, 200, 203, 245, 335–343, 351, 413, 416 |
| `src/config/interpolate.rs` | 29 | 260 | 88.85% | 56, 94, 112–114, 122–124, 135, 145, 201, 217–220, 240–242, 248–251, 268, 328–331, 359 |
| `src/repl/engine/adapter.rs` | 29 | 261 | 88.89% | 117, 171–174, 176–183, 185, 187–192, 269, 317, 330, 332, 371, 387, 390 |
| `src/app/dispatch.rs` | 28 | 235 | 88.09% | 24–26, 42–47, 49, 96–99, 164, 202–213, 255 |
| `src/command_words.rs` | 28 | 116 | 75.86% | 21–22, 41, 64, 115, 133–134, 136–138, 140, 144–159, 161 |
| `src/startup_trace.rs` | 28 | 144 | 80.56% | 108, 131, 138, 148, 163, 170–171, 176, 188, 196, 199–207, 210–211, 217–221, 231–232 |
| `src/completion/engine.rs` | 27 | 407 | 93.37% | 229–236, 242, 245, 297–302, 306, 347, 359, 362, 376, 383, 396 |
| `src/ui/settings/mod.rs` | 27 | 435 | 93.79% | 67–68, 168, 172, 178, 181, 306, 310, 315–320, 322, 334, 518, 558, 567–569, 571, 574, 651, 676, 751 |
| `src/core/runtime.rs` | 26 | 188 | 86.17% | 99, 128, 133, 157, 174, 176–177, 264–278, 280, 349, 447 |
| `src/app/config_explain.rs` | 24 | 472 | 94.92% | 84, 106, 130, 369, 395, 438, 440–446, 453, 475 |
| `src/app/logging.rs` | 24 | 184 | 86.96% | 108, 175–176, 199, 214–217, 234–236, 241, 243, 267, 275, 284, 296, 312, 326 |
| `src/core/document.rs` | 24 | 450 | 94.67% | 33–34, 36–37, 42–47, 49, 202, 291, 310, 446, 485, 541, 574, 577, 591, 617, 703 |
| `src/dsl/engine.rs` | 24 | 268 | 91.04% | 133–136, 140–141, 157, 213, 218, 221, 274, 375–378, 381–382, 408, 422–424, 428, 433 |
| `src/ui/emit/terminal.rs` | 22 | 759 | 97.10% | 153, 161–162, 164–165, 167–168, 294, 461, 539, 568, 629, 645, 713, 757–761, 763, 823 |
| `src/repl/input.rs` | 21 | 207 | 89.86% | 47, 49, 51, 92–93, 95–98, 207–209, 211–212, 254, 261, 267 |
| `src/repl/surface.rs` | 21 | 411 | 94.89% | 63, 67–71, 106, 112, 137–143, 228, 284, 288, 313, 342, 534 |
| `src/config/keys.rs` | 20 | 44 | 54.55% | 15–20, 22–25, 32–34, 36, 44, 71–72, 79 |
| `src/lib.rs` | 20 | 20 | 0.00% | 277–280, 282–285, 287–294, 296–297 |
| `src/app/builtin.rs` | 19 | 132 | 85.61% | 34, 52–56, 75, 128–132, 136, 138, 141, 152, 178, 200 |
| `src/dsl/parse/lexer.rs` | 19 | 277 | 93.14% | 53–55, 58, 64, 157, 195–196, 251, 279, 294–295, 297–298, 321, 327, 333, 372, 384 |
| `src/repl/dispatch/command.rs` | 19 | 352 | 94.60% | 58–59, 178, 180–185, 223, 241, 248, 266–267, 275, 279, 457–458 |
| `src/repl/bell.rs` | 18 | 30 | 40.00% | 17–19, 35–38, 40–45, 48–49 |
| `src/ui/emit/markdown.rs` | 18 | 209 | 91.39% | 14, 25, 32–36, 38, 40, 42–43, 71, 213, 220, 242, 263, 269 |
| `src/core/context.rs` | 17 | 17 | 0.00% | 58–67, 69–75 |
| `src/core/style.rs` | 17 | 111 | 84.68% | 71, 89–91, 112, 119–121, 124–127, 130–131, 133, 168–169 |
| `src/repl/completion.rs` | 17 | 334 | 94.91% | 120–123, 164, 182–183, 275, 281, 287, 290, 293, 296, 409, 429, 447–448 |
| `src/ui/plan/table.rs` | 17 | 231 | 92.64% | 64–66, 68, 75, 95, 132–135, 160–162, 164, 166, 238, 253 |
| `src/app/command_plan.rs` | 16 | 87 | 81.61% | 42, 63–65, 89–92, 99, 118–119 |
| `src/dsl/verbs/aggregate.rs` | 16 | 188 | 91.49% | 60, 75–78, 230, 255, 269, 298, 304, 324–326 |
| `src/dsl/verbs/unroll.rs` | 15 | 182 | 91.76% | 53, 88, 98, 123, 147, 151, 172, 184, 190, 208–210, 245–246, 270 |
| `src/repl/engine/debug.rs` | 15 | 220 | 93.18% | 84–87, 103, 291, 298, 303, 308, 313, 317–320 |
| `src/ui/clipboard/backend.rs` | 15 | 113 | 86.73% | 18–22, 26–30, 70–71, 75 |
| `src/ui/messages/mod.rs` | 15 | 115 | 86.96% | 13–14, 16–18, 24, 27–29, 35, 54, 71–72, 75 |
| `src/app/bootstrap.rs` | 14 | 150 | 90.67% | 152–153, 169, 180, 205, 214–215, 217–218, 227–228, 230 |
| `src/dsl/eval/flatten.rs` | 14 | 104 | 86.54% | 43–44, 58, 65, 77–78, 93, 108, 117, 138, 144, 152, 158, 166 |
| `src/repl/engine/overlay.rs` | 14 | 230 | 93.91% | 84, 95, 97–99, 113, 117–118, 219, 227, 232 |
| `src/app/facts.rs` | 13 | 124 | 89.52% | 34–39, 134–136, 138, 144, 158–159 |
| `src/core/command_policy.rs` | 12 | 66 | 81.82% | 66–71, 132, 147, 162, 164, 168, 171 |
| `src/plugin/discovery.rs` | 12 | 207 | 94.20% | 76, 87, 134, 147–148, 178, 246, 249, 255 |
| `src/repl/dispatch/shell.rs` | 12 | 224 | 94.64% | 86, 125–132, 259, 273, 325 |
| `src/dsl/verbs/jq.rs` | 10 | 125 | 92.00% | 53, 58, 122, 133, 137, 175–176, 182 |
| `src/dsl/verbs/selector.rs` | 10 | 118 | 91.53% | 136, 164, 166–170, 172, 189, 207 |
| `src/ui/style/mod.rs` | 10 | 171 | 94.15% | 110–114, 210–213, 291 |
| `src/completion/parse.rs` | 8 | 410 | 98.05% | 99–100, 301 |
| `src/dsl/parse/path.rs` | 8 | 144 | 94.44% | 114, 130, 149, 156, 166, 191, 201, 219 |
| `src/plugin/env.rs` | 8 | 51 | 84.31% | 42, 60, 65–66, 68–70 |
| `src/app/session.rs` | 7 | 248 | 97.18% | 60, 67, 268, 445, 508, 604 |
| `src/dsl/compiled.rs` | 7 | 93 | 92.47% | 96, 102, 105–106, 123, 131, 137 |
| `src/dsl/parse/pipeline.rs` | 7 | 79 | 91.14% | 44, 75, 81, 120–122, 126 |
| `src/dsl/verbs/quick.rs` | 7 | 220 | 96.82% | 126, 258, 262, 264, 266–267, 292 |
| `src/core/command_error.rs` | 6 | 29 | 79.31% | 36, 54–58 |
| `src/core/document/template.rs` | 6 | 100 | 94.00% | 26, 46, 70, 95, 110, 124 |
| `src/cli/commands/plugins.rs` | 5 | 96 | 94.79% | 47, 77, 80, 99 |
| `src/dsl/verbs/project.rs` | 5 | 166 | 96.99% | 153, 174, 208, 231, 265 |
| `src/ui/emit/key_value.rs` | 5 | 27 | 81.48% | 7–8, 32 |
| `src/ui/messages/render.rs` | 5 | 217 | 97.70% | 59, 100, 163–165 |
| `src/config/error.rs` | 4 | 89 | 95.51% | 60, 302, 312 |
| `src/core/output.rs` | 4 | 64 | 93.75% | 94–95, 203 |
| `src/dsl/verbs/limit.rs` | 4 | 51 | 92.16% | 17, 21 |
| `src/plugin/manager.rs` | 4 | 99 | 95.96% | 36 |
| `src/app/access.rs` | 3 | 25 | 88.00% | 61–63 |
| `src/repl/engine/config.rs` | 3 | 187 | 98.40% | 88, 90 |
| `src/ui/chrome/mod.rs` | 3 | 67 | 95.52% | 45–47 |
| `src/ui/clipboard/mod.rs` | 3 | 76 | 96.05% | 130, 133 |
| `src/app/assembly.rs` | 2 | 285 | 99.30% | 82, 89 |
| `src/app/timing.rs` | 2 | 58 | 96.55% | 34, 44 |
| `src/completion/context.rs` | 2 | 47 | 95.74% | 62, 98 |
| `src/core/clap_metadata.rs` | 2 | 41 | 95.12% | 47, 54 |
| `src/core/command_scope.rs` | 2 | 36 | 94.44% | 72 |
| `src/ui/emit/grid.rs` | 2 | 118 | 98.31% | 129, 143 |
| `src/ui/emit/json.rs` | 2 | 36 | 94.44% | 28 |
| `src/completion/paths.rs` | 1 | 28 | 96.43% | — |
| `src/config/selector.rs` | 1 | 71 | 98.59% | 88 |
| `src/core/output_model.rs` | 1 | 97 | 98.97% | 344 |
| `src/core/shell_words.rs` | 1 | 91 | 98.90% | 153 |
| `src/dsl/eval/numeric.rs` | 1 | 15 | 93.33% | 12 |
| `src/dsl/verb_info.rs` | 1 | 30 | 96.67% | — |
| `src/dsl/verbs/common.rs` | 1 | 61 | 98.36% | — |
| `src/dsl/verbs/json.rs` | 1 | 8 | 87.50% | 8 |
| `src/dsl/verbs/question.rs` | 1 | 18 | 94.44% | 42 |
| `src/repl/engine.rs` | 1 | 93 | 98.92% | 233 |
| `src/ui/emit/shared.rs` | 1 | 23 | 95.65% | 28 |
| `src/ui/emit/table.rs` | 1 | 53 | 98.11% | 66 |
| `src/bin/osp.rs` | 0 | 2 | 100.00% | — |
| `src/cli/commands/history.rs` | 0 | 7 | 100.00% | — |
| `src/cli/commands/intro.rs` | 0 | 27 | 100.00% | — |
| `src/cli/commands/mod.rs` | 0 | 23 | 100.00% | — |
| `src/cli/rows/output.rs` | 0 | 64 | 100.00% | — |
| `src/cli/rows/row.rs` | 0 | 13 | 100.00% | — |
| `src/config/builtin.rs` | 0 | 390 | 100.00% | — |
| `src/config/defaults.rs` | 0 | 33 | 100.00% | — |
| `src/config/explain.rs` | 0 | 37 | 100.00% | — |
| `src/core/fuzzy.rs` | 0 | 30 | 100.00% | — |
| `src/dsl/model.rs` | 0 | 48 | 100.00% | — |
| `src/dsl/parse/key_spec.rs` | 0 | 36 | 100.00% | — |
| `src/dsl/parse/quick.rs` | 0 | 73 | 100.00% | — |
| `src/dsl/verbs/collapse.rs` | 0 | 14 | 100.00% | — |
| `src/dsl/verbs/values.rs` | 0 | 26 | 100.00% | — |
| `src/normalize.rs` | 0 | 19 | 100.00% | — |
| `src/plugin/catalog.rs` | 0 | 23 | 100.00% | — |
| `src/ui/emit/entries.rs` | 0 | 38 | 100.00% | — |
| `src/ui/emit/mod.rs` | 0 | 10 | 100.00% | — |

## Boundary promises and can-fail evidence

Each row was run alone through `scripts/run-hermetic-cargo.py -- cargo test
--locked --test TARGET FILTER -- --test-threads=1`. Each mutation produced
`test result: FAILED. 0 passed; 1 failed` for the named test, rather than a
compile error or empty selection. Every production mutation was restored in a
`finally` block before the next case. The final suites ran on restored code.

| Promise / test filter | Layer | Temporary production mutation | Observed failure |
| --- | --- | --- | --- |
| `read_subject_uses_context` | integration / public resolution API | Disable omitted read filling | Arguments were empty instead of `remembered.example` |
| `explicit_marker_expands` | integration / public resolution API | Retain `.` instead of the remembered value | `--host=.` differed from `--host=chosen.example` |
| `singleton_response_selects` | integration / public resolution API | Disable successful canonical-field selection | Host selection was missing |
| `declared_creation_context_survives` | integration / public resolution API | Ignore declared response context acknowledgements | Created host selection was missing after follow-up failure |
| `service_selections_share` | integration / public session API | Disable shared-kind lookup across services | Shared host selection was missing |
| `missing_read_entity_clears` | integration / public resolution API | Clear an unrelated kind after not-found | Retired host selection remained |
| `narrowed_empty_result_preserves` | integration / public resolution API | Suppress narrow-scope retry hints | Explicit retry hint was missing from messages |
| `opted_in_context_survives` | integration / embedded App in plain subprocesses | Register no persisted kinds | Private store contained no selected host |
| `snapshots_round_trip` | integration / public snapshot API | Pass JSON `null` to the product writer | Matching snapshot could not load |
| `refresh_coalesces` | integration / public refresh API | Lose the in-flight force request | Second callback channel disconnected |
| `adapter_evidence_obeys` | integration / App native adapter | Retain URL userinfo instead of the authority | Expected sanitized service context differed |
| `recording_persists` | contracts / built binary with piped stdin | Stop collecting response secrets | Mirrored response token leaked into recorded JSON |
| `config_picker_stages` | e2e / PTY | Save original instead of staged value | Saved config value remained 0 instead of 2 |
| `theme_picker_saves` | e2e / PTY | Save under `Wrong Name` | Active theme differed from `boundary-theme` |

Recording asserts parsed JSON Lines grouped under the submitted `source`
command, synthetic credential redaction and mode 0600. Context persistence
asserts parsed private storage and mode 0600 across two process lifetimes.
Picker results are read back as parsed JSON after returning to the live editor.
Refresh scheduling uses channels; picker screen waits use reader notifications
and a condition variable. No new test synchronizes with sleeps.

## Caller audit and overlap removal

No production code was proven dead in the selected gaps. The public context
and snapshot APIs are live product dependencies: see product
`src/integrations/mod.rs`, `src/integrations/orch/mod.rs` and
`src/integrations/siteadmin/hostnames.rs`. Diagnostic events are submitted by product `src/auth/service.rs`,
`src/integrations/orch/credential.rs` and `src/integrations/nh/client.rs`. Framework recording, config editing and theme preview
are reachable REPL commands; their new tests execute those routes.
The all-zero snapshot and picker files therefore could not be deleted merely
because the original framework suite did not reach them.

Deleted `registered_command_executes_through_registry_unit` from
`src/native/tests.rs`. Its retained owner is
`app::host::app_host_surfaces_native_commands_in_help_and_dispatch` in
`tests/integration/app/host.rs`, which checks native registration, visible help
and parsed dispatch output through the App boundary. Tests with distinct local
invariants or progress behavior remain.

## Final verification

Final full LLVM coverage: **33,313 / 38,863 = 85.7191%**, versus
**32,065 / 38,863 = 82.5078%** before. Net gain: **1,248 covered lines**;
the executable denominator and the 85% gate are unchanged. The first coverage
round reached 33,315 lines; splitting the two combined context promises produced
the final result above. There are 14 new tests and one overlapping test removed.

Passed on the final source:

- `python3 scripts/confidence.py local`: compiler-backed public docs, rustdoc
  warnings, fmt, clippy, toolchain consistency, architecture, contracts and
  integration.
- `python3 scripts/confidence.py --check coverage`: 863 library cases,
  123 contracts, 23 e2e cases, 51 integration cases and the unit verb-policy
  case, all successful. This includes the Linux native-keyring contract.
- `confidence.check_coverage_exclusions(Path.cwd())`: passed, also invoked
  automatically by the coverage runner. Every new test module is immediately
  test-gated before its nightly `coverage(off)` attribute.
- `git diff --check`: passed. Production mutations were absent from the final
  diff. New PTY scenarios were also killed again after the helper cleanup;
  split context scenarios each have their own failing mutation evidence.

All Cargo runs used `CARGO_TARGET_DIR=/home/oistes/.cache/osp-target/framework-coverage`;
the hermetic runner kept stable outputs under the checkout-name child and
coverage kept pinned nightly objects separate. Scratch and coverage JSON were
kept below the same task cache, which was removed after verification.
The signed branch is local only; no merge or push was performed.

This establishes local Linux behavior with synthetic fixtures, including real
terminal and native-keyring boundaries. The product was audited for callers;
its tests and live service operations were outside this test-only change.

Largest final file gains, from the same native LLVM summaries:

| File | Additional covered lines | Final covered / executable |
| --- | ---: | ---: |
| `src/context.rs` | 226 | 413 / 526 |
| `src/cli/commands/config/picker.rs` | 166 | 166 / 193 |
| `src/ui/picker.rs` | 162 | 162 / 222 |
| `src/diagnostics.rs` | 136 | 213 / 283 |
| `src/cli/commands/theme/preview.rs` | 133 | 133 / 157 |
| `src/repl/recording.rs` | 126 | 178 / 253 |
| `src/snapshot.rs` | 56 | 56 / 62 |
| `src/cli/commands/config.rs` | 50 | 867 / 1068 |
| `src/native.rs` | 33 | 479 / 626 |
| `src/ui/theme_catalog.rs` | 31 | 372 / 398 |
| `src/cli/commands/context.rs` | 24 | 24 / 64 |
| `src/core/context.rs` | 17 | 17 / 17 |
| `src/cli/commands/theme.rs` | 14 | 170 / 208 |
| `src/ui/text.rs` | 13 | 233 / 273 |
| `src/repl/lifecycle.rs` | 12 | 237 / 273 |
