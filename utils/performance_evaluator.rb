# frozen_string_literal: true

require 'net/http'
require 'json'
require 'rest-client'

SUBSTRATE_SERVER = ENV['SUBSTRATE_SERVER']
STRATUS_ROCKS_SERVER = ENV['STRATUS_ROCKS_SERVER']
STRATUS_POSTGRES_SERVER = ENV['STRATUS_POSTGRES_SERVER']

def generate_get_block_by_number_json_body(block_number)
  {
    "jsonrpc": '2.0',
    "method": 'eth_getBlockByNumber',
    "params": ["0x#{block_number.to_s(16)}", true],
    "id": 1
  }.to_json
end

def get_block_by_number(server, block_number)
  uri = URI(server)
  response = RestClient.post(uri.to_s, generate_get_block_by_number_json_body(block_number), content_type: :json)
  print "#{response.body.size}, "
  JSON.parse(response.body)
end

def generate_get_transaction_receipt_json_body(tx_hash)
  {
    "jsonrpc": '2.0',
    "method": 'eth_getTransactionReceipt',
    "params": [tx_hash],
    "id": 1
  }.to_json
end

def get_transaction_receipt(server, tx_hash)
  uri = URI(server)
  response = RestClient.post(uri.to_s, generate_get_transaction_receipt_json_body(tx_hash), content_type: :json)
  print "#{response.body.size}, "
  JSON.parse(response.body)
end

# Example usage
# block_number = 36_000_000
tx_hash = '0x502461a31fd274feff859a592253521cf0242c16d44253cb78027dce12286f98'

require 'time'

$total_block_time = Hash.new(0)
$total_block_count = Hash.new(0)
$total_receipt_time = Hash.new(0)
$total_receipt_count = Hash.new(0)

def measure_block_time(server)
  start_time = Time.now
  yield
  end_time = Time.now

  $total_block_time[server] += (end_time - start_time) * 1000.0
  $total_block_count[server] += 1

  puts "Time spent: #{((end_time - start_time) * 1000.0).round(2)} ms"
end

def measure_receipt_time(server)
  start_time = Time.now
  yield
  end_time = Time.now

  $total_receipt_time[server] += (end_time - start_time) * 1000.0
  $total_receipt_count[server] += 1

  puts "Time spent: #{((end_time - start_time) * 1000.0).round(2)} ms"
end

block_numbers = [
  292_973,
  9_000_057,
  12_000_001,
  15_000_000,
  18_000_105,
  21_000_000,
  24_000_000,
  27_000_000,
  30_000_008,
  33_000_000,
  36_000_000,
  39_000_000,
  42_000_000,
  45_000_000,
  48_000_000,
  52_000_000,
  55_000_000,
  58_000_000,
  61_000_000,
  64_000_000
]

block_numbers.each do |block_number|
  loop do
    substrate_block = get_block_by_number(SUBSTRATE_SERVER, block_number)
    tx_hash = substrate_block['result']['transactions'][0]
    tx_hash = tx_hash['hash'] unless tx_hash.nil?
    break unless tx_hash.nil?

    block_number += 1
  end

  puts
  puts
  puts 'Measuring time spent to get block and transaction receipt from Substrate,'\
       'Stratus Postgres and Stratus Rocks servers'
  puts
  puts '* Substrate'
  print "Block number: #{block_number}, "
  measure_block_time('substrate') { get_block_by_number(SUBSTRATE_SERVER, block_number) }
  print "Transaction hash: #{tx_hash}, "
  measure_receipt_time('substrate') { get_transaction_receipt(SUBSTRATE_SERVER, tx_hash) }

  # puts
  # puts "* Stratus Rocks"
  # print "Block number: #{block_number}, "
  # stratus_rocks_block = measure_block_time { get_block_by_number(STRATUS_ROCKS_SERVER, block_number) }
  # print "Transaction hash: #{tx_hash}, "
  # stratus_rocks_receipt = measure_receipt_time { get_transaction_receipt(STRATUS_ROCKS_SERVER, tx_hash) }

  puts
  puts '* Stratus Postgres'
  print "Block number: #{block_number}, "
  measure_block_time('postgre') do
    get_block_by_number(STRATUS_POSTGRES_SERVER, block_number)
  end
  print "Transaction hash: #{tx_hash}, "
  measure_receipt_time('postgre') do
    get_transaction_receipt(STRATUS_POSTGRES_SERVER, tx_hash)
  end
end

puts '*****'
