/*
 * Copyright (c) 2015 Cryptonomex, Inc., and contributors.
 *
 * The MIT License
 *
 * Permission is hereby granted, free of charge, to any person obtaining a copy
 * of this software and associated documentation files (the "Software"), to deal
 * in the Software without restriction, including without limitation the rights
 * to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
 * copies of the Software, and to permit persons to whom the Software is
 * furnished to do so, subject to the following conditions:
 *
 * The above copyright notice and this permission notice shall be included in
 * all copies or substantial portions of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 * IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 * FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
 * AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
 * LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 * OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
 * THE SOFTWARE.
 */
#include <graphene/app/application.hpp>
#include <graphene/app/plugin.hpp>
#include <graphene/app/config_util.hpp>

#include <graphene/chain/balance_object.hpp>

#include <graphene/utilities/tempdir.hpp>

#include <graphene/account_history/account_history_plugin.hpp>
#include <graphene/market_history/market_history_plugin.hpp>
#include <graphene/witness/witness.hpp>
#include <graphene/grouped_orders/grouped_orders_plugin.hpp>

#include <fc/thread/thread.hpp>
#include <fc/log/appender.hpp>
#include <fc/log/logger.hpp>

#include <boost/filesystem/path.hpp>
#include <boost/asio/io_context.hpp>
#include <boost/asio/ip/udp.hpp>
#include <boost/asio/ip/tcp.hpp>

#define BOOST_TEST_MODULE Test Application
#include <boost/test/included/unit_test.hpp>

#include "../common/genesis_file_util.hpp"

using namespace graphene;
namespace bpo = boost::program_options;

namespace fc {
   extern std::unordered_map<std::string, logger> &get_logger_map();
   extern std::unordered_map<std::string, appender::ptr> &get_appender_map();
}

/// Waits until @p condition holds or @p timeout passes; returns whether it holds.
/// Fixed sleeps made the P2P tests depend on how fast the machine happens to be.
template< typename Condition >
static bool wait_for( const Condition& condition, fc::microseconds timeout = fc::seconds(10) )
{
   const fc::time_point deadline = fc::time_point::now() + timeout;
   while( !condition() )
   {
      if( fc::time_point::now() >= deadline )
         return false;
      fc::usleep( fc::milliseconds(50) );
   }
   return true;
}

BOOST_AUTO_TEST_CASE(load_configuration_options_test_config_logging_files_created)
{
   fc::temp_directory app_dir(graphene::utilities::temp_directory_path());
   auto dir = app_dir.path();
   auto config_ini_file = dir / "config.ini";
   auto logging_ini_file = dir / "logging.ini";

   /// create default config options
   auto node = new app::application();
   bpo::options_description cli, cfg;
   node->set_program_options(cli, cfg);
   bpo::options_description cfg_options("Graphene Witness Node");
   cfg_options.add(cfg);

   /// check preconditions
   BOOST_CHECK(!fc::exists(config_ini_file));
   BOOST_CHECK(!fc::exists(logging_ini_file));

   bpo::variables_map options;
   app::load_configuration_options(dir, cfg_options, options);

   /// check post-conditions
   BOOST_CHECK(fc::exists(config_ini_file));
   BOOST_CHECK(fc::exists(logging_ini_file));
   BOOST_CHECK_GT(fc::file_size(config_ini_file), 0u);
   BOOST_CHECK_GT(fc::file_size(logging_ini_file), 0u);
}

BOOST_AUTO_TEST_CASE(load_configuration_options_test_config_ini_options)
{
   fc::temp_directory app_dir(graphene::utilities::temp_directory_path());
   auto dir = app_dir.path();
   auto config_ini_file = dir / "config.ini";
   auto logging_ini_file = dir / "logging.ini";

   /// create config.ini
   bpo::options_description cfg_options("config.ini options");
   cfg_options.add_options()
   ("option1", bpo::value<std::string>(), "")
   ("option2", bpo::value<int>(), "")
   ;
   std::ofstream out(config_ini_file.preferred_string());
   out << "option1=is present\n"
          "option2=1\n\n";
   out.close();

   /// check preconditions
   BOOST_CHECK(fc::exists(config_ini_file));
   BOOST_CHECK(!fc::exists(logging_ini_file));

   bpo::variables_map options;
   app::load_configuration_options(dir, cfg_options, options);

   /// check the options values are parsed into the output map
   BOOST_CHECK(!options.empty());
   BOOST_CHECK_EQUAL(options.count("option1"), 1u);
   BOOST_CHECK_EQUAL(options.count("option2"), 1u);
   BOOST_CHECK_EQUAL(options["option1"].as<std::string>(), "is present");
   BOOST_CHECK_EQUAL(options["option2"].as<int>(), 1);

   /// when the config.ini exists and doesn't contain logging configuration while the logging.ini doesn't exist
   /// the logging.ini is not created
   BOOST_CHECK(!fc::exists(logging_ini_file));
}

BOOST_AUTO_TEST_CASE(load_configuration_options_test_logging_ini_options)
{
   fc::temp_directory app_dir(graphene::utilities::temp_directory_path());
   auto dir = app_dir.path();
   auto config_ini_file = dir / "config.ini";
   auto logging_ini_file = dir / "logging.ini";

   /// create logging.ini
   /// configure exactly one logger and appender
   std::ofstream out(logging_ini_file.preferred_string());
   out << "[log.file_appender.default]\n"
          "filename=test.log\n\n"
          "[logger.default]\n"
          "level=info\n"
          "appenders=default\n\n"
          ;
   out.close();

   /// clear logger and appender state
   fc::get_logger_map().clear();
   fc::get_appender_map().clear();
   BOOST_CHECK(fc::get_logger_map().empty());
   BOOST_CHECK(fc::get_appender_map().empty());

   bpo::options_description cfg_options("empty");
   bpo::variables_map options;
   app::load_configuration_options(dir, cfg_options, options);

   /// check the options values are parsed into the output map
   /// this is a little bit tricky since load_configuration_options() doesn't provide output variable for logging_config
   auto logger_map = fc::get_logger_map();
   auto appender_map = fc::get_appender_map();
   BOOST_CHECK_EQUAL(logger_map.size(), 1u);
   BOOST_CHECK(logger_map.count("default"));
   BOOST_CHECK_EQUAL(appender_map.size(), 1u);
   BOOST_CHECK(appender_map.count("default"));
}

BOOST_AUTO_TEST_CASE(load_configuration_options_test_legacy_config_ini_options)
{
   fc::temp_directory app_dir(graphene::utilities::temp_directory_path());
   auto dir = app_dir.path();
   auto config_ini_file = dir / "config.ini";
   auto logging_ini_file = dir / "logging.ini";

   /// create config.ini
   bpo::options_description cfg_options("config.ini options");
   cfg_options.add_options()
   ("option1", bpo::value<std::string>(), "")
   ("option2", bpo::value<int>(), "")
   ;
   std::ofstream out(config_ini_file.preferred_string());
   out << "option1=is present\n"
          "option2=1\n\n"
          "[log.file_appender.default]\n"
          "filename=test.log\n\n"
          "[logger.default]\n"
          "level=info\n"
          "appenders=default\n\n"
          ;
   out.close();

   /// clear logger and appender state
   fc::get_logger_map().clear();
   fc::get_appender_map().clear();
   BOOST_CHECK(fc::get_logger_map().empty());
   BOOST_CHECK(fc::get_appender_map().empty());

   bpo::variables_map options;
   app::load_configuration_options(dir, cfg_options, options);

   /// check logging.ini not created
   BOOST_CHECK(!fc::exists(logging_ini_file));

   /// check the options values are parsed into the output map
   BOOST_CHECK(!options.empty());
   BOOST_CHECK_EQUAL(options.count("option1"), 1u);
   BOOST_CHECK_EQUAL(options.count("option2"), 1u);
   BOOST_CHECK_EQUAL(options["option1"].as<std::string>(), "is present");
   BOOST_CHECK_EQUAL(options["option2"].as<int>(), 1);

   auto logger_map = fc::get_logger_map();
   auto appender_map = fc::get_appender_map();
   BOOST_CHECK_EQUAL(logger_map.size(), 1u);
   BOOST_CHECK(logger_map.count("default"));
   BOOST_CHECK_EQUAL(appender_map.size(), 1u);
   BOOST_CHECK(appender_map.count("default"));
}

/////////////
/// @brief create a 2 node network
/////////////
BOOST_AUTO_TEST_CASE( two_node_network )
{
   using namespace graphene::chain;
   using namespace graphene::app;
   try {
      BOOST_TEST_MESSAGE( "Creating and initializing app1" );

      fc::temp_directory app_dir( graphene::utilities::temp_directory_path() );
      // One genesis file for both nodes. create_example_genesis() takes the current time rounded
      // down to the block interval, so two separate calls can fall on either side of a boundary,
      // give the nodes different chain ids and keep them from connecting.
      const boost::filesystem::path genesis_file = create_genesis_file( app_dir );

      graphene::app::application app1;
      app1.register_plugin< graphene::account_history::account_history_plugin>();
      app1.register_plugin< graphene::market_history::market_history_plugin >();
      app1.register_plugin< graphene::witness_plugin::witness_plugin >();
      app1.register_plugin< graphene::grouped_orders::grouped_orders_plugin>();
      app1.startup_plugins();
      boost::program_options::variables_map cfg;
      cfg.emplace("p2p-endpoint", boost::program_options::variable_value(string("127.0.0.1:3939"), false));
      cfg.emplace("genesis-json", boost::program_options::variable_value(genesis_file, false));
      cfg.emplace("seed-nodes", boost::program_options::variable_value(string("[]"), false));
      app1.initialize(app_dir.path(), cfg);
      BOOST_TEST_MESSAGE( "Starting app1 and waiting 500 ms" );
      app1.startup();
      fc::usleep(fc::milliseconds(500));

      BOOST_TEST_MESSAGE( "Creating and initializing app2" );

      fc::temp_directory app2_dir( graphene::utilities::temp_directory_path() );
      graphene::app::application app2;
      app2.register_plugin<account_history::account_history_plugin>();
      app2.register_plugin< graphene::market_history::market_history_plugin >();
      app2.register_plugin< graphene::witness_plugin::witness_plugin >();
      app2.register_plugin< graphene::grouped_orders::grouped_orders_plugin>();
      app2.startup_plugins();
      auto cfg2 = cfg;
      cfg2.erase("p2p-endpoint");
      // app2 listens on a random port. The node binds its outbound connections to its own listening
      // port, and app2 closes first, so a fixed port stayed in TIME_WAIT for a minute after each run
      // and the next run's app2 kept waiting for it to become available.
      cfg2.emplace("p2p-endpoint", boost::program_options::variable_value(string("127.0.0.1:0"), false));
      cfg2.emplace("seed-node", boost::program_options::variable_value(vector<string>{"127.0.0.1:3939"}, false));
      cfg2.emplace("seed-nodes", boost::program_options::variable_value(string("[]"), false));
      app2.initialize(app2_dir.path(), cfg2);

      BOOST_TEST_MESSAGE( "Starting app2 and waiting for the nodes to connect" );
      app2.startup();
      wait_for( [&app1] { return app1.p2p_node()->get_connection_count() == 1u; } );

      BOOST_REQUIRE_EQUAL(app1.p2p_node()->get_connection_count(), 1u);
      BOOST_CHECK_EQUAL(std::string(app1.p2p_node()->get_connected_peers().front().host.get_address()), "127.0.0.1");
      BOOST_TEST_MESSAGE( "app1 and app2 successfully connected" );

      std::shared_ptr<chain::database> db1 = app1.chain_database();
      std::shared_ptr<chain::database> db2 = app2.chain_database();

      BOOST_CHECK_EQUAL( db1->get_balance( GRAPHENE_NULL_ACCOUNT, asset_id_type() ).amount.value, 0 );
      BOOST_CHECK_EQUAL( db2->get_balance( GRAPHENE_NULL_ACCOUNT, asset_id_type() ).amount.value, 0 );

      BOOST_TEST_MESSAGE( "Creating transfer tx" );
      graphene::chain::precomputable_transaction trx;
      {
         account_id_type nathan_id = db2->get_index_type<account_index>().indices().get<by_name>().find( "nathan" )->id;
         fc::ecc::private_key nathan_key = fc::ecc::private_key::regenerate(fc::sha256::hash(string("nathan")));

         balance_claim_operation claim_op;
         balance_id_type bid = balance_id_type();
         claim_op.deposit_to_account = nathan_id;
         claim_op.balance_to_claim = bid;
         claim_op.balance_owner_key = nathan_key.get_public_key();
         claim_op.total_claimed = bid(*db1).balance;
         trx.operations.push_back( claim_op );
         db1->current_fee_schedule().set_fee( trx.operations.back() );

         transfer_operation xfer_op;
         xfer_op.from = nathan_id;
         xfer_op.to = GRAPHENE_NULL_ACCOUNT;
         xfer_op.amount = asset( 1000000 );
         trx.operations.push_back( xfer_op );
         db1->current_fee_schedule().set_fee( trx.operations.back() );

         trx.set_expiration( db1->get_slot_time( 10 ) );
         trx.sign( nathan_key, db1->get_chain_id() );
         trx.validate();
      }

      BOOST_TEST_MESSAGE( "Pushing tx locally on db1" );
      processed_transaction ptrx = db1->push_transaction(trx);

      BOOST_CHECK_EQUAL( db1->get_balance( GRAPHENE_NULL_ACCOUNT, asset_id_type() ).amount.value, 1000000 );
      BOOST_CHECK_EQUAL( db2->get_balance( GRAPHENE_NULL_ACCOUNT, asset_id_type() ).amount.value, 0 );

      BOOST_TEST_MESSAGE( "Broadcasting tx" );
      // Right after the handshake app2 may still be syncing with app1, and the node drops new
      // inventory for a peer that needs sync items from it instead of advertising it later.
      // Repeat the broadcast until app2 has the transaction; an item is advertised to a peer once.
      for( int attempt = 0; attempt < 20; ++attempt )
      {
         app1.p2p_node()->broadcast(graphene::net::trx_message(trx));
         if( wait_for( [&db2] { return db2->get_balance( GRAPHENE_NULL_ACCOUNT, asset_id_type() ).amount.value == 1000000; },
                       fc::milliseconds(500) ) )
            break;
      }

      BOOST_CHECK_EQUAL( db1->get_balance( GRAPHENE_NULL_ACCOUNT, asset_id_type() ).amount.value, 1000000 );
      BOOST_CHECK_EQUAL( db2->get_balance( GRAPHENE_NULL_ACCOUNT, asset_id_type() ).amount.value, 1000000 );

      BOOST_TEST_MESSAGE( "Generating block on db2" );
      fc::ecc::private_key committee_key = fc::ecc::private_key::regenerate(fc::sha256::hash(string("nathan")));

      auto block_1 = db2->generate_block(
         db2->get_slot_time(1),
         db2->get_scheduled_witness(1),
         committee_key,
         database::skip_nothing);

      BOOST_TEST_MESSAGE( "Broadcasting block" );
      app2.p2p_node()->broadcast(graphene::net::block_message( block_1 ));

      wait_for( [&app1] { return app1.chain_database()->head_block_num() == 1u; } );
      BOOST_TEST_MESSAGE( "Verifying nodes are still connected" );
      BOOST_CHECK_EQUAL(app1.p2p_node()->get_connection_count(), 1u);
      BOOST_CHECK_EQUAL(app1.chain_database()->head_block_num(), 1u);

      BOOST_TEST_MESSAGE( "Checking GRAPHENE_NULL_ACCOUNT has balance" );
   } catch( fc::exception& e ) {
      edump((e.to_detail_string()));
      throw;
   }
}

/// A local address that is not loopback, or an empty string if the machine has none.
/// Connects a UDP socket (which sends nothing) to a documentation address to learn the source
/// address the system would use.
static std::string non_loopback_local_address()
{
   try
   {
      boost::asio::io_context io;
      boost::asio::ip::udp::socket socket( io );
      socket.connect( boost::asio::ip::udp::endpoint( boost::asio::ip::make_address("192.0.2.1"), 9 ) );
      boost::asio::ip::address address = socket.local_endpoint().address();
      if( address.is_v4() && !address.is_loopback() )
         return address.to_string();
   }
   catch( const std::exception& ) {}
   return std::string();
}

/// Connecting to a peer while an earlier connection to it is not closed yet.
/// The node binds its outbound connections to its own listening port. While an earlier
/// connection between the same two endpoints still exists (an unanswered FIN keeps it in
/// FIN_WAIT for minutes when the network is down, which is when a node loses all its peers),
/// connect() fails with "Cannot assign requested address" (EADDRNOTAVAIL). The node has to fall
/// back to a port the OS picks instead of counting the peer as failed and retrying a minute later.
/// A socket from the same local endpoint to the same peer, held open by the test, stands in
/// for the earlier connection. The addresses are not loopback, where Linux treats connections
/// more leniently.
BOOST_AUTO_TEST_CASE( reconnect_from_same_port_after_disconnect )
{
   using namespace graphene::chain;
   using namespace graphene::app;
   try {
      const std::string host = non_loopback_local_address();
      if( host.empty() )
      {
         BOOST_TEST_MESSAGE( "No non-loopback address, skipping" );
         return;
      }
      fc::temp_directory app1_dir( graphene::utilities::temp_directory_path() );
      const boost::filesystem::path genesis_file = create_genesis_file( app1_dir );

      auto make_config = [&genesis_file]( const string& endpoint, const vector<string>& seed_nodes ) {
         bpo::variables_map cfg;
         cfg.emplace("p2p-endpoint", bpo::variable_value(endpoint, false));
         cfg.emplace("genesis-json", bpo::variable_value(genesis_file, false));
         cfg.emplace("seed-nodes", bpo::variable_value(string("[]"), false));
         if( !seed_nodes.empty() )
            cfg.emplace("seed-node", bpo::variable_value(seed_nodes, false));
         return cfg;
      };

      // application keeps a pointer to its options: they have to outlive it
      const bpo::variables_map cfg1 = make_config(host + ":3940", {});
      graphene::app::application app1;
      app1.startup_plugins();
      app1.initialize(app1_dir.path(), cfg1);
      app1.startup();
      // app1 only accepts: if it connected to app2 itself, that connection would hide the problem
      app1.p2p_node()->set_advanced_node_parameters( fc::mutable_variant_object()
                                                     ("desired_number_of_connections", 0) );

      // app2 without seed nodes first: its listener has to own the port before another socket
      // binds to it (the node itself binds its outbound sockets the same way)
      fc::temp_directory app2_dir( graphene::utilities::temp_directory_path() );
      const bpo::variables_map cfg2 = make_config(host + ":3941", {});
      graphene::app::application app2;
      app2.startup_plugins();
      app2.initialize(app2_dir.path(), cfg2);
      app2.startup();

      BOOST_TEST_MESSAGE( "An earlier connection " + host + ":3941 -> " + host + ":3940 that is not closed" );
      boost::asio::io_context io;
      boost::asio::ip::tcp::socket earlier_connection( io );
      earlier_connection.open( boost::asio::ip::tcp::v4() );
      earlier_connection.set_option( boost::asio::socket_base::reuse_address(true) );
#ifdef SO_REUSEPORT
      earlier_connection.set_option( boost::asio::detail::socket_option::boolean<SOL_SOCKET, SO_REUSEPORT>(true) );
#endif
      const auto address = boost::asio::ip::make_address( host );
      earlier_connection.bind( boost::asio::ip::tcp::endpoint( address, 3941 ) );
      earlier_connection.connect( boost::asio::ip::tcp::endpoint( address, 3940 ) );

      BOOST_TEST_MESSAGE( "app2 connects to app1 from " + host + ":3941" );
      app2.p2p_node()->connect_to_endpoint( fc::ip::endpoint::from_string( host + ":3940" ) );
      // Well under the retry delay after a failed attempt
      BOOST_CHECK( wait_for( [&] { return app2.p2p_node()->get_connection_count() == 1u; }, fc::seconds(15) ) );
      earlier_connection.close();
   } catch( fc::exception& e ) {
      BOOST_FAIL( e.to_detail_string() );
   }
}

// a contrived example to test the breaking out of application_impl to a header file

#include "../../libraries/app/application_impl.hxx"

BOOST_AUTO_TEST_CASE(application_impl_breakout) {
   class test_impl : public graphene::app::detail::application_impl {
      // override the constructor, just to test that we can
   public:
      test_impl() : application_impl(nullptr) {}
      bool has_item(const net::item_id& id) override {
         return true;
      }
   };

   test_impl impl;
   graphene::net::item_id id;
   BOOST_CHECK(impl.has_item(id));
}

/// Options of a test node for the P2P tests: no default seed nodes, a fixed listening endpoint
static bpo::variables_map p2p_test_config( const boost::filesystem::path& genesis_file, const std::string& endpoint,
                                           const std::vector<std::string>& seed_nodes,
                                           const std::string& tls_mode = "auto" )
{
   bpo::variables_map cfg;
   cfg.emplace("p2p-endpoint", bpo::variable_value(endpoint, false));
   cfg.emplace("genesis-json", bpo::variable_value(genesis_file, false));
   cfg.emplace("seed-nodes", bpo::variable_value(std::string("[]"), false));
   cfg.emplace("p2p-tls", bpo::variable_value(tls_mode, false));
   if( !seed_nodes.empty() )
      cfg.emplace("seed-node", bpo::variable_value(seed_nodes, false));
   return cfg;
}

BOOST_AUTO_TEST_CASE( seed_node_address_parsing )
{
   using graphene::app::detail::parse_seed_node_address;

   auto plain_ip = parse_seed_node_address( "1.2.3.4:1776" );
   BOOST_CHECK_EQUAL( plain_ip.host_and_port, "1.2.3.4:1776" );
   BOOST_CHECK( !plain_ip.tls );
   BOOST_CHECK( !plain_ip.server_name ); // no SNI for an IP address

   auto plain_name = parse_seed_node_address( " seed.example.org:1776 " );
   BOOST_CHECK_EQUAL( plain_name.host_and_port, "seed.example.org:1776" );
   BOOST_CHECK( !plain_name.tls );
   BOOST_REQUIRE( plain_name.server_name );
   BOOST_CHECK_EQUAL( *plain_name.server_name, "seed.example.org" );

   auto tls_ip = parse_seed_node_address( "TLS://1.2.3.4:1776" );
   BOOST_CHECK_EQUAL( tls_ip.host_and_port, "1.2.3.4:1776" );
   BOOST_CHECK( tls_ip.tls );
   BOOST_CHECK( !tls_ip.server_name );

   auto tls_sni = parse_seed_node_address( "tls://1.2.3.4:1776?sni=example.org" );
   BOOST_CHECK_EQUAL( tls_sni.host_and_port, "1.2.3.4:1776" );
   BOOST_CHECK( tls_sni.tls );
   BOOST_REQUIRE( tls_sni.server_name );
   BOOST_CHECK_EQUAL( *tls_sni.server_name, "example.org" );

   BOOST_CHECK_THROW( parse_seed_node_address( "tls://1.2.3.4:1776?foo=bar" ), fc::exception );
}

/// Starts a node listening on 127.0.0.1:port, connecting to the given seed nodes
struct p2p_test_node
{
   fc::temp_directory            dir{ graphene::utilities::temp_directory_path() };
   bpo::variables_map            config; // application keeps a pointer to it: declared before app
   graphene::app::application    app;

   /// Listens on a port the OS picks: fixed ports may be taken on a CI machine
   p2p_test_node( const boost::filesystem::path& genesis_file,
                  const std::vector<std::string>& seed_nodes, const std::string& tls_mode ) :
      config( p2p_test_config( genesis_file, "127.0.0.1:0", seed_nodes, tls_mode ) )
   {
      app.startup_plugins();
      app.initialize( dir.path(), config );
      app.startup();
   }

   uint32_t connections() { return app.p2p_node()->get_connection_count(); }

   /// "127.0.0.1:port" this node listens on
   std::string address() { return std::string( app.p2p_node()->get_actual_listening_endpoint() ); }

   /// "plain", or the TLS version, of the only connection
   std::string transport()
   {
      auto peers = app.p2p_node()->get_connected_peers();
      BOOST_REQUIRE_EQUAL( peers.size(), 1u );
      return peers.front().info["transport"].as_string();
   }

   fc::optional<net::potential_peer_record> peer_record( const std::string& endpoint )
   {
      for( const auto& record : app.p2p_node()->get_potential_peers() )
         if( std::string(record.endpoint) == endpoint )
            return record;
      return {};
   }
};

/// Two new nodes connect over TLS: the client tries it, the server tells it from the first byte
BOOST_AUTO_TEST_CASE( p2p_tls_between_new_nodes )
{
   try {
      fc::temp_directory genesis_dir( graphene::utilities::temp_directory_path() );
      const boost::filesystem::path genesis_file = create_genesis_file( genesis_dir );

      p2p_test_node server( genesis_file, {}, "auto" );
      p2p_test_node client( genesis_file, { server.address() }, "auto" );
      BOOST_REQUIRE( wait_for( [&] { return server.connections() == 1u && client.connections() == 1u; } ) );

      BOOST_CHECK_NE( server.transport(), "plain" );
      BOOST_CHECK_NE( client.transport(), "plain" );
      BOOST_CHECK_EQUAL( client.transport().substr(0, 3), "TLS" );
      auto record = client.peer_record( server.address() );
      BOOST_REQUIRE( record );
      BOOST_CHECK( record->transport == net::transport_tls );
   } catch( fc::exception& e ) {
      BOOST_FAIL( e.to_detail_string() );
   }
}

/// An old client (TLS off behaves like one) connects to a new node plainly on the same port
BOOST_AUTO_TEST_CASE( p2p_tls_old_client_to_new_server )
{
   try {
      fc::temp_directory genesis_dir( graphene::utilities::temp_directory_path() );
      const boost::filesystem::path genesis_file = create_genesis_file( genesis_dir );

      p2p_test_node server( genesis_file, {}, "auto" );
      p2p_test_node client( genesis_file, { server.address() }, "off" );
      BOOST_REQUIRE( wait_for( [&] { return server.connections() == 1u && client.connections() == 1u; } ) );

      BOOST_CHECK_EQUAL( server.transport(), "plain" );
      BOOST_CHECK_EQUAL( client.transport(), "plain" );
   } catch( fc::exception& e ) {
      BOOST_FAIL( e.to_detail_string() );
   }
}

/// A new client tries TLS with an old node, sees the plain protocol in the answer, reconnects
/// plainly and remembers the node as plain
BOOST_AUTO_TEST_CASE( p2p_tls_new_client_falls_back_for_old_server )
{
   try {
      fc::temp_directory genesis_dir( graphene::utilities::temp_directory_path() );
      const boost::filesystem::path genesis_file = create_genesis_file( genesis_dir );

      p2p_test_node server( genesis_file, {}, "off" );
      p2p_test_node client( genesis_file, { server.address() }, "auto" );
      BOOST_REQUIRE( wait_for( [&] { return server.connections() == 1u && client.connections() == 1u; } ) );

      BOOST_CHECK_EQUAL( server.transport(), "plain" );
      BOOST_CHECK_EQUAL( client.transport(), "plain" );
      auto record = client.peer_record( server.address() );
      BOOST_REQUIRE( record );
      BOOST_CHECK( record->transport == net::transport_plain );
   } catch( fc::exception& e ) {
      BOOST_FAIL( e.to_detail_string() );
   }
}

/// With TLS "always" a client does not fall back to the plain protocol
BOOST_AUTO_TEST_CASE( p2p_tls_always_does_not_fall_back )
{
   try {
      fc::temp_directory genesis_dir( graphene::utilities::temp_directory_path() );
      const boost::filesystem::path genesis_file = create_genesis_file( genesis_dir );

      p2p_test_node server( genesis_file, {}, "off" );
      p2p_test_node client( genesis_file, { server.address() }, "always" );
      BOOST_CHECK( !wait_for( [&] { return client.connections() > 0u; }, fc::seconds(5) ) );
      BOOST_CHECK_EQUAL( server.connections(), 0u );
   } catch( fc::exception& e ) {
      BOOST_FAIL( e.to_detail_string() );
   }
}
