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
#include <boost/multi_index_container.hpp>
#include <boost/multi_index/ordered_index.hpp>
#include <boost/multi_index/hashed_index.hpp>
#include <boost/multi_index/member.hpp>
#include <boost/multi_index/mem_fun.hpp>
#include <boost/multi_index/tag.hpp>

#include <fc/io/raw.hpp>
#include <fc/io/raw_variant.hpp>
#include <fc/log/logger.hpp>
#include <fc/io/json.hpp>

#include <graphene/net/peer_database.hpp>
#include <graphene/net/config.hpp>

namespace graphene { namespace net {
  namespace detail
  {
    using namespace boost::multi_index;

    class peer_database_impl
    {
    public:
      struct last_seen_time_index {};
      struct endpoint_index {};
      typedef boost::multi_index_container<potential_peer_record, 
                                           indexed_by<ordered_non_unique<tag<last_seen_time_index>, 
                                                                         member<potential_peer_record, 
                                                                                fc::time_point_sec, 
                                                                                &potential_peer_record::last_seen_time> >,
                                                      hashed_unique<tag<endpoint_index>, 
                                                                    member<potential_peer_record, 
                                                                           fc::ip::any_endpoint, 
                                                                           &potential_peer_record::endpoint>, 
                                                                    std::hash<fc::ip::any_endpoint> > > > potential_peer_set;

    private:
      potential_peer_set     _potential_peer_set;
      fc::path _peer_database_filename;
      bool _is_open = false;

    public:
      void open(const fc::path& databaseFilename);
      void close();
      void clear();
      void erase(const fc::ip::any_endpoint& endpointToErase);
      void load_file(const fc::path& file_name);
      void save_file(const fc::path& file_name, const std::vector<potential_peer_record>& records);
      void update_entry(const potential_peer_record& updatedRecord);
      potential_peer_record lookup_or_create_entry_for_endpoint(const fc::ip::any_endpoint& endpointToLookup);
      fc::optional<potential_peer_record> lookup_entry_for_endpoint(const fc::ip::any_endpoint& endpointToLookup);

      peer_database::iterator begin() const;
      peer_database::iterator end() const;
      size_t size() const;
    };

    class peer_database_iterator_impl
    {
    public:
      typedef peer_database_impl::potential_peer_set::index<peer_database_impl::last_seen_time_index>::type::iterator last_seen_time_index_iterator;
      last_seen_time_index_iterator _iterator;
      explicit peer_database_iterator_impl(const last_seen_time_index_iterator& iterator) :
        _iterator(iterator)
      {}
    };
    peer_database_iterator::peer_database_iterator( const peer_database_iterator& c ) :
      boost::iterator_facade<peer_database_iterator, const potential_peer_record, boost::forward_traversal_tag>(c){}

    namespace
    {
      // IPv6 peers go to a file of their own: a node of 1.2.1 or older reading peers.json
      // fails on the first address it cannot parse and starts with an empty database, so
      // peers.json keeps IPv4 addresses only and stays readable after a downgrade
      fc::path ipv6_file_name(const fc::path& peer_database_filename)
      {
        return peer_database_filename.parent_path() / "peers_v6.json";
      }
    }

    void peer_database_impl::load_file(const fc::path& file_name)
    {
      if (!fc::exists(file_name))
        return;
      try
      {
        // record by record: a record that does not parse is skipped, not the whole file
        const fc::variants records = fc::json::from_file(file_name).get_array();
        for (const fc::variant& record : records)
        {
          try
          {
            potential_peer_record peer = record.as<potential_peer_record>(GRAPHENE_NET_MAX_NESTED_OBJECTS);
            // not kept any more (see close()); a file of an earlier build may have them
            if (!peer.endpoint.get_address().is_nat64())
              _potential_peer_set.insert(peer);
          }
          catch (const fc::exception& e)
          {
            wlog("skipping a record of the peer database ${file}: ${e}", ("file", file_name)("e", e.to_string()));
          }
        }
      }
      catch (const fc::exception& e)
      {
        elog("error opening peer database file ${peer_database_filename}, starting with a clean database", 
             ("peer_database_filename", file_name));
      }
    }

    void peer_database_impl::save_file(const fc::path& file_name, const std::vector<potential_peer_record>& records)
    {
      try
      {
        fc::path dir = file_name.parent_path();
        if (!fc::exists(dir))
          fc::create_directories(dir);
        fc::json::save_to_file( records, file_name, GRAPHENE_NET_MAX_NESTED_OBJECTS );
      }
      catch (const fc::exception& e)
      {
        elog("error saving peer database to file ${peer_database_filename}", 
             ("peer_database_filename", file_name));
      }
    }

    void peer_database_impl::open(const fc::path& peer_database_filename)
    {
      _peer_database_filename = peer_database_filename;
      _is_open = true;
      load_file(_peer_database_filename);
      load_file(ipv6_file_name(_peer_database_filename));
      if (_potential_peer_set.size() > MAXIMUM_PEERDB_SIZE)
      {
        // prune database to a reasonable size
        auto iter = _potential_peer_set.begin();
        std::advance(iter, MAXIMUM_PEERDB_SIZE);
        _potential_peer_set.erase(iter, _potential_peer_set.end());
      }
    }

    void peer_database_impl::close()
    {
      // close() empties the in-memory set after saving it, so a second call would overwrite
      // the file with an empty list. The P2P node is closed more than once on shutdown
      // (application::shutdown(), then ~application() and ~node_impl()), so only the first
      // call may write.
      if (!_is_open)
        return;
      _is_open = false;

      std::vector<potential_peer_record> v4_records;
      std::vector<potential_peer_record> v6_records;
      v4_records.reserve(_potential_peer_set.size());
      for (const potential_peer_record& record : _potential_peer_set)
      {
        // A NAT64 address (64:ff9b::a.b.c.d, from a DNS64 resolver) is the way to an IPv4 node through the
        // gateway of the network we are on now; on another network it leads nowhere. The node gets it again
        // by resolving the seed where it is needed, so it is not kept.
        if (record.endpoint.get_address().is_nat64())
          continue;
        (record.endpoint.is_v6() ? v6_records : v4_records).push_back(record);
      }

      save_file(_peer_database_filename, v4_records);
      const fc::path v6_file = ipv6_file_name(_peer_database_filename);
      if (!v6_records.empty() || fc::exists(v6_file))
        save_file(v6_file, v6_records);
      _potential_peer_set.clear();
    }

    void peer_database_impl::clear()
    {
      _potential_peer_set.clear();
    }

    void peer_database_impl::erase(const fc::ip::any_endpoint& endpointToErase)
    {
      auto iter = _potential_peer_set.get<endpoint_index>().find(endpointToErase);
      if (iter != _potential_peer_set.get<endpoint_index>().end())
        _potential_peer_set.get<endpoint_index>().erase(iter);
    }

    void peer_database_impl::update_entry(const potential_peer_record& updatedRecord)
    {
      auto iter = _potential_peer_set.get<endpoint_index>().find(updatedRecord.endpoint);
      if (iter != _potential_peer_set.get<endpoint_index>().end())
        _potential_peer_set.get<endpoint_index>().modify(iter, [&updatedRecord](potential_peer_record& record) { record = updatedRecord; });
      else
        _potential_peer_set.get<endpoint_index>().insert(updatedRecord);
    }

    potential_peer_record peer_database_impl::lookup_or_create_entry_for_endpoint(const fc::ip::any_endpoint& endpointToLookup)
    {
      auto iter = _potential_peer_set.get<endpoint_index>().find(endpointToLookup);
      if (iter != _potential_peer_set.get<endpoint_index>().end())
        return *iter;
      return potential_peer_record(endpointToLookup);
    }

    fc::optional<potential_peer_record> peer_database_impl::lookup_entry_for_endpoint(const fc::ip::any_endpoint& endpointToLookup)
    {
      auto iter = _potential_peer_set.get<endpoint_index>().find(endpointToLookup);
      if (iter != _potential_peer_set.get<endpoint_index>().end())
        return *iter;
      return fc::optional<potential_peer_record>();
    }

    peer_database::iterator peer_database_impl::begin() const
    {
      return peer_database::iterator(new peer_database_iterator_impl(_potential_peer_set.get<last_seen_time_index>().begin()));
    }

    peer_database::iterator peer_database_impl::end() const
    {
      return peer_database::iterator(new peer_database_iterator_impl(_potential_peer_set.get<last_seen_time_index>().end()));
    }

    size_t peer_database_impl::size() const
    {
      return _potential_peer_set.size();
    }

    peer_database_iterator::peer_database_iterator()
    {
    }

    peer_database_iterator::~peer_database_iterator()
    {
    }

    peer_database_iterator::peer_database_iterator(peer_database_iterator_impl* impl) :
      my(impl)
    {
    }

    void peer_database_iterator::increment()
    {
      ++my->_iterator;
    }

    bool peer_database_iterator::equal(const peer_database_iterator& other) const
    {
      return my->_iterator == other.my->_iterator;
    }

    const potential_peer_record& peer_database_iterator::dereference() const
    {
      return *my->_iterator;
    }

  } // end namespace detail

  peer_database::peer_database() :
    my(new detail::peer_database_impl)
  {
  }

  peer_database::~peer_database()
  {}

  void peer_database::open(const fc::path& databaseFilename)
  {
    my->open(databaseFilename);
  }

  void peer_database::close()
  {
    my->close();
  }

  void peer_database::clear()
  {
    my->clear();
  }

  void peer_database::erase(const fc::ip::any_endpoint& endpointToErase)
  {
    my->erase(endpointToErase);
  }

  void peer_database::update_entry(const potential_peer_record& updatedRecord)
  {
    my->update_entry(updatedRecord);
  }

  potential_peer_record peer_database::lookup_or_create_entry_for_endpoint(const fc::ip::any_endpoint& endpointToLookup)
  {
    return my->lookup_or_create_entry_for_endpoint(endpointToLookup);
  }

  fc::optional<potential_peer_record> peer_database::lookup_entry_for_endpoint(const fc::ip::any_endpoint& endpoint_to_lookup)
  {
    return my->lookup_entry_for_endpoint(endpoint_to_lookup);
  }

  peer_database::iterator peer_database::begin() const
  {
    return my->begin();
  }

  peer_database::iterator peer_database::end() const
  {
    return my->end();
  }

  size_t peer_database::size() const
  {
    return my->size();
  }

} } // end namespace graphene::net

FC_REFLECT_ENUM( graphene::net::potential_peer_last_connection_disposition,
                 (never_attempted_to_connect)
                 (last_connection_failed)(last_connection_rejected)
                 (last_connection_handshaking_failed)(last_connection_succeeded) )
FC_REFLECT_ENUM( graphene::net::potential_peer_transport,
                 (transport_unknown)(transport_plain)(transport_tls) )
FC_REFLECT_DERIVED_NO_TYPENAME( graphene::net::potential_peer_record, BOOST_PP_SEQ_NIL,
                                (endpoint)(last_seen_time)(last_connection_disposition)
                                (last_connection_attempt_time)(number_of_successful_connection_attempts)
                                (number_of_failed_connection_attempts)(last_error)
                                (transport)(tls_server_name)(alternate_endpoints)(alternate_endpoints_confirmed) )

GRAPHENE_IMPLEMENT_EXTERNAL_SERIALIZATION( graphene::net::potential_peer_record)
