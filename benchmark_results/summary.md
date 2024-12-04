| Config Tested                                         | Max TPS | Avg TPS | Avg Read Time Per Slot | Avg Time to Save Block |
| ----------------------------------------------------- | ------- | ------- | ---------------------- | ---------------------- |
| main                                                  | 1274    | 1164    | 41.8us                 | 115ms                  |
| minimal                                               | 1709    | 1276    | 36.5us                 | 122ms                  |
| OptimizeForPointLookup                                | 1655    | 1284    | 33.5us                 | 121ms                  |
| Prefix Extractor                                      | 1655    | 1286    | 32.4us                 | 120ms                  |
| Row Cache                                             | 1676    | 1286    | 32.7us                 | 125ms                  |
| HashSearch Index Type                                 | 1695    | 1288    | 33us                   | 123ms                  |
| Direct IO                                             | 1718    | 1298    | 32.5us                 | 128ms                  |
| HashSkipList memtable                                 | 1716    | 1284    | 34.6us                 | 205ms                  |
| Configs After this point are based on DirectIO config |         |         |                        |                        |
| PlainTable                                            | 1689    | 1300    | 34.4us                 | 122ms                  |
| Small Hash Ratio                                      | 1713    | 1315    | 32.1us                 | 121ms                  |
| Large Hash Ratio                                      | 1713    | 1334    | 31.6us                 | 123ms                  |
| Large PrefixBloom Ratio                               | 1746    | 1302    | 34.6us                 | 122ms                  |
| Small Block Size                                      | 1693    | 1340    | 31.2us                 | 124ms                  |
| Large Block Size                                      | 1693    | 1306    | 32.9us                 | 123ms                  |
| Hyperclock Cache                                      | 1677    | 1333    | 31.9us                 | 124ms                  |
