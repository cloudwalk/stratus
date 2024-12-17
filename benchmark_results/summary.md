| Config Tested                                                                    | Max TPS | Avg TPS | Avg Read Time Per Slot | Avg Time to Save Block |
| -------------------------------------------------------------------------------- | ------- | ------- | ---------------------- | ---------------------- |
| main                                                                             | 1135    | 1029    | 41.8us                 | 131ms                  |
| minimal                                                                          | 1480    | 1092    | 41.6us                 | 126ms                  |
| OptimizeForPointLookup                                                           | 1567    | 1200    | 38.3us                 | 133ms                  |
| Prefix Extractor                                                                 | 1558    | 1171    | 38.8us                 | 127ms                  |
| Row Cache                                                                        | 1498    | 1136    | 38.1us                 | 125ms                  |
| HashSearch Index Type                                                            | 1575    | 1204    | 38.8us                 | 140ms                  |
| Direct IO                                                                        | 1536    | 1148    | 38.5us                 | 123ms                  |
| HashSkipList memtable                                                            | 1544    | 1197    | 38.5us                 | 197ms                  |
| _Configs After this point are based on DirectIO config_                          |         |         |                        |                        |
| PlainTable                                                                       | 1498    | 1134    | 38.8us                 | 126ms                  |
| Small Hash Ratio                                                                 | 1508    | 1201    | 38.4us                 | 132ms                  |
| Large Hash Ratio                                                                 | 1505    | 1136    | 38.8us                 | 131ms                  |
| Large PrefixBloom Ratio                                                          | 1554    | 1152    | 39.8us                 | 133ms                  |
| Small Block Size                                                                 | 1378    | 1039    | 40.0us                 | 117ms                  |
| Large Block Size                                                                 | 1339    | 1019    | 41.0us                 | 124ms                  |
| Hyperclock Cache                                                                 | 1709    | 1153    | 38.4us                 | 132ms                  |
| _Configs After this point are the final configs created using the results above_ |         |         |                        |                        |
| final_01                                                                         | 1521    | 1111    | 39.4us                 | 137ms                  |
| final_02                                                                         | 1517    | 1091    | 38.3us                 | 144ms                  |
| final_03                                                                         | 1552    | 1126    | 39.1us                 | 146ms                  |
| -------------------------------------------------------------------------------- | ------- | ------- | ---------------------- | ---------------------- |

It was observed that rocksdb metrics (collected by rocks itself) were having a huge impact on point-lookups and adding a lot of variance, so the final tests were rerun
without collecting those metrics

| Config Tested                                                                    | Max TPS | Avg TPS | Avg Read Time Per Slot | Avg Time to Save Block |
| -------------------------------------------------------------------------------- | ------- | ------- | ---------------------- | ---------------------- |
| final_01                                                                         | 1464    | 1249    | 6.53us                 | 122ms                  |
| final_02                                                                         | 1461    | 1271    | 6.77us                 | 113ms                  |
| final_03                                                                         | 1541    | 1298    | 6.45us                 | 113ms                  |
| -------------------------------------------------------------------------------- | ------- | ------- | ---------------------- | ---------------------- |

A lot of the differences between the tests can be attributed to variance, so final_03 was selected as the configuration candidate for having
a sensible configuration that should increase performance, stability and reliability, besides being much simpler than what we have today.
I truly believe that this config is very close to optimal in terms of striking a balance between point-lookup performance and stability+reliability.

As an extra some other tests were done by changing the VM + disk type:

| Config Tested                                                              | Max TPS | Avg TPS | Avg Read Time Per Slot | Avg Time to Save Block |
| -------------------------------------------------------------------------- | ------- | ------- | ---------------------- | ---------------------- |
| n2-standard-128 (intel ice lake) + extreme disk 120000 IOPS                | 1694    | 1483    | 4.98us                 | 115ms                  |
| n4 + hyperdisk balanced                                                    | 1551    | 1443    | 5.30us                 | 109ms                  |
| n4 + hyperdisk balanced (metrics disaled)                                  | 1656    | 1473    |                        |                        |
| n4 + hyperdisk balanced (logs level warn)                                  | 1938    | 1716    | 4.79us                 | 130ms                  |
| n4 + hyperdisk balanced (metrics disabled and logs level warn)             | 2143    | 1893    |                        |                        |
| n4 + hyperdisk balanced (metrics and tracing disabled and logs level warn) | 2200    | 1926    |                        |                        |
| -------------------------------------------------------------------------- | ------- | ------- | ---------------------- | ---------------------- |
