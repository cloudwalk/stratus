| Config Tested                                         | Max TPS | Avg TPS | Avg Read Time Per Slot | Avg Time to Save Block |
| ----------------------------------------------------- | ------- | ------- | ---------------------- | ---------------------- |
| main                                                  | 1135    | 1029    | 41.8us                 | 131ms                  |
| minimal                                               | 1480    | 1092    | 41.6us                 | 126ms                  |
| OptimizeForPointLookup                                | 1567    | 1200    | 38.3us                 | 133ms                  |
| Prefix Extractor                                      | 1558    | 1171    | 38.8us                 | 127ms                  |
| Row Cache                                             | 1498    | 1136    | 38.1us                 | 125ms                  |
| HashSearch Index Type                                 | 1575    | 1204    | 38.8us                 | 140ms                  |
| Direct IO                                             | 1536    | 1148    | 38.5us                 | 123ms                  |
| HashSkipList memtable                                 | 1544    | 1197    | 38.5us                 | 197ms                  |
| Configs After this point are based on DirectIO config |         |         |                        |                        |
| PlainTable                                            | 1498    | 1134    | 38.8us                 | 126ms                  |
| Small Hash Ratio                                      | 1508    | 1201    | 38.4us                 | 132ms                  |
| Large Hash Ratio                                      | 1505    | 1136    | 38.8us                 | 131ms                  |
| Large PrefixBloom Ratio                               | 1554    | 1152    | 39.8us                 | 133ms                  |
| Small Block Size                                      | 1378    | 1039    | 40.0us                 | 117ms                  |
| Large Block Size                                      | 1339    | 1019    | 41.0us                 | 124ms                  |
| Hyperclock Cache                                      | 1709    | 1253    | 38.4us                 | 132ms                  |
