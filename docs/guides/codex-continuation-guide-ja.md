# CodexCont 推論の自動継続

CodexCont は、途中で切れた複数のネイティブ Codex Responses ストリームを、クライアントから見える 1 本のストリームに結合します。タスクが終わっていないのに、一定の内部 reasoning token 境界で停止するプロバイダー向けの機能です。

## 有効化と設定

**設定 → ルーティング → CodexCont** を開きます。既定値は有効、最大 8 回、ステップ 518、継続マーカーは次の内容です。

> We need continue thinking. Do not summarize; continue from the previous reasoning state.

最大回数は費用と安全の上限であり、目標回数ではありません。通常どおり完了すれば上流リクエストは 1 回です。実際に継続するたびに課金対象となり得る上流リクエストが 1 回増え、遅延と token 使用量も増えます。

次の環境変数は、現在のプロセスで保存済み設定を上書きします。

| 環境変数 | 内容 |
| --- | --- |
| `CCSWITCH_CODEX_CONTINUE` | `true`/`false`、`1`/`0`、`on`/`off` |
| `CCSWITCH_CODEX_CONTINUE_MAX` | 最大継続回数 |
| `CCSWITCH_CODEX_CONTINUE_STEP` | 切断フィンガープリントのステップ。3 未満は 3 に補正 |
| `CCSWITCH_CODEX_CONTINUE_MARKER` | 継続指示。空の値は無視 |

環境変数を変更した後は CC Switch を再起動してください。

## 動作条件

次の条件をすべて満たす必要があります。

1. ストリーミング `/v1/responses` リクエストである。
2. reasoning が明示的に無効化されていない。
3. 最初から最後までネイティブ Responses 経路である。Responses→Chat と Responses→Anthropic の変換経路は対象外。
4. compact リクエストではない。
5. 終端 usage の reasoning token 数が設定した切断フィンガープリントに一致する。既定ステップでは `518 × n - 2` が一致。
6. バッファ済み出力にツールまたはアクション項目がない。`function_call`、`custom_tool_call`、`local_shell_call`、不明な型は継続を止め、クライアントが実行すべき操作を失わないようにする。
7. 最大継続回数に達していない。

条件を満たすと、CC Switch は前の応答出力を引き継ぎ、`reasoning.encrypted_content` を要求し、マーカーを追加して、同じ `RequestForwarder::forward_with_retry` から再送します。プロバイダー選択、再試行、フェイルオーバー、ヘッダー、メトリクス、クォータ帰属は通常のリクエストと共通です。

## ストリーム結合と安全性

クライアントには 1 本の SSE ストリームだけが届きます。CodexCont は区間をまたいで sequence と output index を振り直し、usage を合算し、終端応答を 1 回だけ送ります。新しいツール呼び出しイベントと従来のストリーミング `function_call` の両方を保持します。

判定は意図的に保守的です。継続を見逃した場合は不完全な回答が見えるので再試行できますが、ツール呼び出し付近で誤って継続すると、クライアントが実行すべき操作を隠す可能性があります。このため message 以外の出力が 1 つでもあれば自動継続しません。

## 調整とトラブルシューティング

- ログと上流 usage が別の境界を継続的に示さない限り、`step = 518` を維持してください。
- 費用と最悪時の遅延を抑えるには `maxContinuations` を下げます。複数回の実際の切断を確認してから増やしてください。
- marker は短く明確にします。変更すると次の上流ターンの挙動も変わります。
- 生のプロバイダー挙動を比較するときは CodexCont を無効にできます。無効でも通常の CC Switch ルーティングは継続します。

継続しない場合は、Codex のプロキシ引き継ぎ、ネイティブ Responses、`stream: true`、reasoning、終端イベントの `usage.output_tokens_details.reasoning_tokens` を確認してください。ツールを生成したターンが継続しないのは正常です。頻繁すぎる場合は既定ステップに戻し、費用や遅延が大きい場合は最大回数を下げてください。
