require "net/http"
require "json"

slack_webhook_url = ENV["SLACK_WEBHOOK_URL"]
github_commit = ENV["GITHUB_SHA"]

slack_message =
  {
    "blocks": [
      {
        "type": "header",
        "text": {
          "type": "plain_text",
          "text": "Stratus deployed on Production",
        },
      },
      {
        "type": "section",
        "fields": [
          {
            "type": "mrkdwn",
            "text": "Commit <https://github.com/cloudwalk/stratus/commit/#{github_commit}|#{github_commit}>",
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
