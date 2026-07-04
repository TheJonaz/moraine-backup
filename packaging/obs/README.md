# openSUSE Build Service (OBS) packaging for Moraine

OBS is the easiest self-service path for openSUSE (and it can also build `.rpm`
and `.deb` for many other distros from one place). It reuses the RPM spec from
`../rpm/moraine.spec`.

## Set up a home project

```sh
osc checkout home:YOURUSER
cd home:YOURUSER
osc mkpac moraine && cd moraine
cp /path/to/moraine-backup/packaging/rpm/moraine.spec .
cp /path/to/moraine-backup/packaging/obs/_service .
osc service manualrun          # downloads the source tarball named in the spec
osc add moraine.spec _service *.tar.gz
osc commit -m "moraine 0.1.19"
```

In the OBS web UI, enable the repositories you want (openSUSE Tumbleweed/Leap,
Fedora, Debian, Ubuntu, …) and OBS builds them all. Enable "network access" for
the project (the build fetches crates from crates.io), or vendor them.

## Into openSUSE Tumbleweed (official)

Once it builds cleanly in your home project, submit it to the distribution:

```sh
osc submitrequest home:YOURUSER moraine openSUSE:Factory
```

A Factory reviewer then accepts or requests changes.

## On each new release

Bump `Version:` in the spec (already tracked in `../rpm/`), re-run
`osc service manualrun`, and `osc commit`.
