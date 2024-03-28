
require 'pg'
require 'json'
require 'byebug'
require 'rest-client'

host = ARGV[0]
user = ARGV[1]
password = ARGV[2]
dbname = ARGV[3]
rpcserver = ARGV[4]
limit = ARGV[5]
compare_slots = ARGV[6]
if compare_slots == nil
    compare_slots = false
else
    compare_slots = compare_slots == 'true' or compare_slots == '1'
end

pg = PG::Connection.connect(host: host, user: user, password: password, dbname: dbname)
print "Executing query..."
a1 = Time.now

# result = pg.query("select * from neo_blocks where block_number <= 22843803 order by block_number desc limit #{limit}").to_a
result = pg.query("select * from neo_blocks order by block_number desc limit #{limit}").to_a
a2 = Time.now
puts "Done #{a2 - a1} seconds"
slots = []
to, from, signer = 0, 0, 0
method, mint_on, value = 0, 0, 0
slots2 = 0

res = []
res_file = File.new('res.txt', 'w')

wallets = {}
wallet_file = File.new('wallets.txt', 'w')

puts "Process #{result.length} blocks"

puts "Processing..."
# loop do
    result.each do |row|
        block_number = row['block_number'].to_i
        puts "Processing block #{block_number}..." if block_number % 1000 == 0
        json = JSON.parse(row['block'])
        json['transactions'].each do |tx|
            transaction_hash = tx['input']['hash']
            slots = []
            to = tx['input']['to']
            from = tx['input']['from']
            signer = tx['input']['signer']

            method = tx['input']['input'][0,34]
            to2 = tx['input']['input'][34,40]
            value = tx['input']['input'][74,74]
            input = tx['input']['input']

            tx['execution']['changes'].each do |change|
                change['slots'].each_pair do |key,value|
                slots << [ key, value['original']['set']['value'], value['modified']['set']['value'] ]
                end
            end

            # what slot is the account balance
            slots2 = nil
            begin
                slots2 = if value != nil && value.to_i(16) > 0
                    slots.select do |slot|
                        # verifies what index 1 summed to value gives index 2
                        if slot[1] != nil && slot[2] != nil
                            original = slot[1].to_i(16)
                            modified = slot[2].to_i(16)
                            value2 = value.to_i(16)
                            summed = original + value2
                            #puts "Key: #{slot[0]} Original: #{original} Modified: #{modified} Value: #{value2} Sum: #{original + value2}"
                            summed == modified
                        else
                            false
                        end
                    end
                else
                    []
                end
            rescue => e
                puts "falha no slot"
                puts e.message
                p json
                p slots
                puts e.message
                byebug
                exit
            end

            r = {transaction_hash: transaction_hash, input: input, block_number: block_number, to: to, from: from, signer: signer, method: method, mint_on: to2, value: value, slots: slots, account_slot: slots2 }
            res << r
            res_file << JSON.dump( r ) + "\n"

            if not slots2.empty?
                wallets[r[:mint_on]] ||= { block_number: 0 }
                if wallets[r[:mint_on]][:block_number] < r[:block_number]
                    w = { transaction_hash: transaction_hash, from: from, to: to, block_number: r[:block_number], value: r[:value], mint_on: r[:mint_on], account_slot: r[:account_slot] }
                    wallets[r[:mint_on]] = w
                end
            end
        end

    end
# end
res_file.close

def get_storage_at(rpcserver, address, slot, block_number)
    # requests get_storage_at ethereum call using restclient
    body = { id: 1, jsonrpc: '2.0', method: 'eth_getStorageAt', params: [ address, slot, block_number ] }
    RestClient.post rpcserver, JSON.dump(body), { content_type: :json, accept: :json }    
end

puts "Size of wallets: #{wallets.size}"
diff = 0
equal = 0

inconsistency_out = File.new('wrong_wallets.txt', 'w')
wallets.each_pair do |k,v|
    wallet_file << JSON.dump(v) + "\n"
    # byebug

    v[:account_slot].each do |slot|
        result = JSON.parse(get_storage_at(rpcserver, v[:to], slot[0], v[:block_number]))['result']
        if result.to_i(16) != slot[2].to_i(16)
            diff += 1
            inconsistency_out << "Inconsistency: transaction_hash: #{v[:transaction_hash]} from account #{v[:from]} to account #{v[:to]} slot index #{slot[0]} block number #{v[:block_number]} slot value #{slot[2]} slot value at mainnet #{result}\n"
        else
            equal += 1
        end
    end
end
inconsistency_out.close
puts "Wallets - Diff: #{diff} Equal: #{equal}"

wallet_file.close

puts "Compare every slot with mainnet..."
diff = 0
equal = 0

wrong_slots = File.new('wrong_slots.txt', 'w')
res.each do |r|
    r[:slots].each do |slot|
        result = JSON.parse(get_storage_at(rpcserver, r[:to], slot[0], r[:block_number]))['result'].to_i(16)
        if result != slot[2].to_i(16)
            wrong_slots << "Inconsistency: tx #{r[:transaction_hash]} account #{r[:from]} account #{r[:to]} slot index #{slot[0]} block number #{r[:block_number]} slot value #{slot[2]} slot value at mainnet #{result}" + "\n"
            diff += 1
        else
            equal += 1
        end
    end
end
wrong_slots.close
puts "All Slots Diff: #{diff} Equal: #{equal}"
puts "Done"