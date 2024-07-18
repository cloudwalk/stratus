require "json"
require "net/http"

def generate_tag_file(tag)
  `fd --type f --extension rs | xargs rg #{tag} --context 0 --line-number --with-filename --color=never --heading > #{tag}.txt`
end

def notify_slack(tag)
  matches = File.read("#{tag}.txt")

  slack_message = {
    "blocks": [
      {
        "type": 'header',
        "text": {
          "type": 'plain_text',
          "text": "#{tag} comments in Stratus Rust files"
        }
      },
      {
        "type": 'rich_text',
        "elements": [
          {
            "type": 'rich_text_preformatted',
            "elements": [
              {
                "type": 'text',
                "text": matches
              }
            ],
            border: 0
          }
        ]
      }
    ]
  }.to_json

  uri = URI(ENV['SLACK_WEBHOOK_URL'])
  http = Net::HTTP.new(uri.host, uri.port)
  http.use_ssl = true
  request = Net::HTTP::Post.new(uri.path, { 'Content-Type' => 'application/json' })
  request.body = slack_message
  response = http.request(request)
  puts response.body
end

def report_tag_comments
  raise "Please provide SLACK_WEBHOOK_URL" if ENV['SLACK_WEBHOOK_URL'].nil?

  tags = %w[TODO FIXME HACK XXX OPTIMIZE BUG]

  tags.each do |tag|
    generate_tag_file(tag)
    next if File.zero? "#{tag}.txt"
    notify_slack(tag)
  end
end

report_tag_comments
