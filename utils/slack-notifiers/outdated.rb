require "net/http"
require "json"

outdated_table_file_name = ENV["OUTDATED_TABLE_FILE_NAME"]
slack_webhook_url = ENV["SLACK_WEBHOOK_URL"]

if outdated_table_file_name.nil? || slack_webhook_url.nil?
  raise "Please provide OUTDATED_TABLE_FILE_NAME and SLACK_WEBHOOK_URL"
end

outdated_table = File.read(outdated_table_file_name)

slack_message = {
  "blocks": [
    {
      "type": "header",
      "text": {
        "type": "plain_text",
        "text": "Stratus outdated dependencies",
      },
    },
    {
      "type": "rich_text",
      "elements": [
        {
          "type": "rich_text_preformatted",
          "elements": [
            {
              "type": "text",
              "text": outdated_table,
            },
          ],
          border: 0,
        },
      ],
    },
  ],
}.to_json

uri = URI(slack_webhook_url)
http = Net::HTTP.new(uri.host, uri.port)
http.use_ssl = true
request = Net::HTTP::Post.new(uri.path, { "Content-Type" => "application/json" })
request.body = slack_message
response = http.request(request)
puts response.body
